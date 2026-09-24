//! 真实 FR 管线冒烟（ignored：需要本机 java + freerouting jar + KiCad pcbnew）。
//!
//! 跑法（Mac 示例）：
//! ```bash
//! KROUTE_TEST_BOARD=/abs/path/board.kicad_pcb \
//! KROUTE_TEST_JAVA=projects/ccd_backend/tools/jre/Contents/Home/bin/java \
//! KROUTE_TEST_JAR=projects/ccd_backend/tools/freerouting-1.9.0.jar \
//! cargo test --test fr_real -- --ignored --nocapture
//! ```

use kroute_server::backends::BackendCfg;
use kroute_server::proto::k_route_server::KRouteServer;
use kroute_server::proto::pb::{Backend, JobState, Strategy, SubmitReq};
use kroute_server::proto::KRouteClient;
use kroute_server::server::{AppState, KRouteImpl};
use kroute_server::store::JobStore;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tonic::transport::Server;
use tonic::Request;

#[tokio::test]
#[ignore = "需要本机 java + freerouting jar + KiCad pcbnew（见文件头说明）"]
async fn fr_real_board_e2e() {
    let board_path = std::env::var("KROUTE_TEST_BOARD").expect("设 KROUTE_TEST_BOARD");
    let java = std::env::var("KROUTE_TEST_JAVA").unwrap_or_else(|_| "java".into());
    let jar = std::env::var("KROUTE_TEST_JAR").expect("设 KROUTE_TEST_JAR");

    let board = std::fs::read(&board_path).expect("读板");
    let tmp = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(JobStore::open(tmp.path().join("jobs")).expect("store"));
    let state = Arc::new(AppState {
        store,
        cfg: Arc::new(BackendCfg {
            java,
            fr_jar: PathBuf::from(&jar),
            kicad_python: if cfg!(target_os = "macos") {
                "/Applications/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/bin/python3"
                    .into()
            } else {
                "/usr/bin/python3".into()
            },
            system_python: "python3".into(),
        }),
        sem: Arc::new(Semaphore::new(1)),
        active: AtomicU64::new(0),
        max_concurrent: 1,
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

    let mut client = KRouteClient::connect(format!("http://{addr}"))
        .await
        .expect("connect");
    let handle = client
        .submit_job(Request::new(SubmitReq {
            backend: Backend::Fr as i32,
            board,
            board_name: PathBuf::from(&board_path)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
            strategy: Some(Strategy {
                router_args: "".into(),
                timeout_secs: 1800,
                keep_traces: false,
            }),
            note: "fr-real-test".into(),
        }))
        .await
        .expect("submit")
        .into_inner();

    let mut stream = client
        .watch(Request::new(kroute_server::proto::pb::JobRef {
            job_id: handle.job_id.clone(),
        }))
        .await
        .expect("watch")
        .into_inner();
    while let Some(p) = stream.message().await.expect("stream") {
        println!("[{:^9?}] {}", p.state(), p.message);
    }
    let fr = client
        .fetch_result(Request::new(kroute_server::proto::pb::JobRef {
            job_id: handle.job_id,
        }))
        .await
        .expect("fetch")
        .into_inner();
    assert_eq!(fr.state(), JobState::Done, "error={}", fr.error);
    assert!(!fr.result_board.is_empty(), "应有布线产物");
    println!("FR 真板管线 DONE，产物 {} bytes", fr.result_board.len());
}
