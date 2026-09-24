//! 端到端冒烟：起真 gRPC 服务（in-proc），提交 noop 任务走完 submit→watch→fetch 全链路。
//! 不依赖 java/kicad——FR 管线的依赖探测由 Health 覆盖，真实 FR 冒烟跑 `cargo test -- --ignored`。

use kroute_server::proto::k_route_server::KRouteServer;
use kroute_server::proto::pb::{
    Backend, FetchReply, HealthReply, HealthReq, JobRef, JobState, ListReq, Strategy, SubmitReq,
};
use kroute_server::proto::KRouteClient;
use kroute_server::server::{AppState, KRouteImpl};
use kroute_server::store::JobStore;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tokio::sync::Semaphore;

const BOARD_MIN: &[u8] = b"(kicad_pcb (version 20241229) (generator kroute-test))\n";

async fn spawn_server() -> String {
    let tmp = tempfile::tempdir().expect("tempdir");
    // tempdir 在函数返回时删除——测试生命周期内保持；用 leak 简化（测试进程短命）
    let jobs_root = tmp.path().join("jobs");
    std::mem::forget(tmp);
    let store = Arc::new(JobStore::open(jobs_root).expect("store"));
    let state = Arc::new(AppState {
        store,
        cfg: Arc::new(kroute_server::backends::BackendCfg {
            java: "java".into(),
            fr_jar: PathBuf::from("definitely-missing.jar"),
            kicad_python: "python3".into(),
            system_python: "python3".into(),
        }),
        sem: Arc::new(Semaphore::new(2)),
        active: AtomicU64::new(0),
        max_concurrent: 2,
        version: "test".into(),
        vram_gate: std::sync::Arc::new(kroute_server::vram::VramGate::new(0)),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        Server::builder()
            .add_service(KRouteServer::new(KRouteImpl { state }))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .expect("serve");
    });
    format!("http://{addr}")
}

use tonic::transport::Server;
use tonic::Request;

fn submit_req(note: &str) -> SubmitReq {
    SubmitReq {
        backend: Backend::Noop as i32,
        board: BOARD_MIN.to_vec(),
        board_name: "t.kicad_pcb".into(),
        strategy: Some(Strategy {
            router_args: "".into(),
            timeout_secs: 30,
            keep_traces: false,
        }),
        note: note.into(),
    }
}

async fn wait_terminal(
    client: &mut KRouteClient<tonic::transport::Channel>,
    job_id: &str,
) -> FetchReply {
    let mut stream = client
        .watch(Request::new(JobRef {
            job_id: job_id.into(),
        }))
        .await
        .expect("watch")
        .into_inner();
    let mut last_state = None;
    while let Some(p) = stream.message().await.expect("stream") {
        last_state = Some(p.state());
    }
    let fr = client
        .fetch_result(Request::new(JobRef {
            job_id: job_id.into(),
        }))
        .await
        .expect("fetch")
        .into_inner();
    assert!(last_state.is_some(), "watch 至少应推送一条");
    fr
}

#[tokio::test]
async fn noop_e2e_submit_watch_fetch() {
    let addr = spawn_server().await;
    let mut client = KRouteClient::connect(addr.clone()).await.expect("connect");

    let handle = client
        .submit_job(Request::new(submit_req("e2e-1")))
        .await
        .expect("submit")
        .into_inner();
    assert!(!handle.reused, "首次提交不应命中幂等");
    assert_eq!(handle.state(), JobState::Queued);

    let fr = wait_terminal(&mut client, &handle.job_id).await;
    assert_eq!(fr.state(), JobState::Done, "error={}", fr.error);
    assert_eq!(fr.result_board, BOARD_MIN, "noop 应原样返回输入板");
    let log_str = String::from_utf8_lossy(&fr.log).to_string();
    assert!(log_str.contains("[run] DONE"), "日志应含 DONE: {log_str}");
}

#[tokio::test]
async fn submit_idempotent_same_payload() {
    let addr = spawn_server().await;
    let mut client = KRouteClient::connect(addr).await.expect("connect");

    let h1 = client
        .submit_job(Request::new(submit_req("idem")))
        .await
        .expect("submit1")
        .into_inner();
    let h2 = client
        .submit_job(Request::new(submit_req("idem")))
        .await
        .expect("submit2")
        .into_inner();
    assert_eq!(h1.job_id, h2.job_id, "同 payload 应同 job_id");
    assert!(h2.reused, "第二次提交应 reused=true");

    // note 参与哈希：不同 note → 不同 job
    let h3 = client
        .submit_job(Request::new(submit_req("idem-diff")))
        .await
        .expect("submit3")
        .into_inner();
    assert_ne!(h1.job_id, h3.job_id);
}

#[tokio::test]
async fn cancel_queued_job() {
    let addr = spawn_server().await;
    let mut client = KRouteClient::connect(addr).await.expect("connect");
    let h = client
        .submit_job(Request::new(submit_req("cancel-me")))
        .await
        .expect("submit")
        .into_inner();
    let r = client
        .cancel(Request::new(JobRef {
            job_id: h.job_id.clone(),
        }))
        .await
        .expect("cancel")
        .into_inner();
    assert!(r.cancelled || r.state() == JobState::Cancelled, "应可取消");
    let fr = wait_terminal(&mut client, &h.job_id).await;
    assert!(
        matches!(fr.state(), JobState::Cancelled | JobState::Done),
        "noop 竞速：取消成功=Cancelled，取消晚于完成=Done，实际 {:?}",
        fr.state()
    );
}

#[tokio::test]
async fn list_jobs_returns_summary() {
    let addr = spawn_server().await;
    let mut client = KRouteClient::connect(addr).await.expect("connect");
    client
        .submit_job(Request::new(submit_req("list-1")))
        .await
        .expect("submit")
        .into_inner();
    let r = client
        .list_jobs(Request::new(ListReq { limit: 10 }))
        .await
        .expect("list")
        .into_inner();
    assert_eq!(r.jobs.len(), 1);
    assert_eq!(r.jobs[0].board_name, "t.kicad_pcb");
}

#[tokio::test]
async fn health_reports_dependencies() {
    let addr = spawn_server().await;
    let mut client = KRouteClient::connect(addr).await.expect("connect");
    let h: HealthReply = client
        .health(Request::new(HealthReq {}))
        .await
        .expect("health")
        .into_inner();
    assert!(!h.ready, "测试环境无 jar，ready 应为 false");
    assert!(h.freerouting_jar.starts_with("ERR"), "缺 jar 应报 ERR");
    assert!(h.max_concurrent >= 1);
}
