#!/bin/sh
# 容器入口：先起虚拟 X server（freerouting 的 AWT 需要 DISPLAY + headful 库），再起服务
Xvfb :99 -screen 0 1024x768x24 &
sleep 1
export DISPLAY=:99
exec kroute-server serve --addr 0.0.0.0:50051 --jobs-dir /data/jobs "$@"
