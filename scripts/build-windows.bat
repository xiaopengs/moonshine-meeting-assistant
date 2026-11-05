@echo off
chcp 65001 >nul
setlocal
echo ============================================================
echo   Moonshine 会议助手 - Windows 本机一键构建
echo ============================================================
echo.
echo 前置要求：
echo   1) Rust 工具链  https://rustup.rs
echo   2) Visual Studio Build Tools（勾选「使用 C++ 的桌面开发」）
echo   3) Windows 10/11（WebView2，Win11 自带）
echo.

where cargo >nul 2>&1 || (
  echo [错误] 未找到 cargo，请先安装 Rust 工具链。
  pause & exit /b 1
)

echo [1/3] 构建音频诊断工具 audio-probe.exe ...
cargo build -p audio-probe --release || (echo [错误] 诊断工具构建失败 & pause & exit /b 1)

echo [2/3] 检查 / 安装 Tauri CLI（首次约 5-15 分钟）...
cargo tauri --version >nul 2>&1 || cargo install tauri-cli --version "^2" --locked || (
  echo [错误] Tauri CLI 安装失败 & pause & exit /b 1
)

echo [3/3] 构建桌面应用（NSIS 安装包）...
cargo tauri build || (echo [错误] 打包失败 & pause & exit /b 1)

echo.
echo ========================== 产物 ==========================
echo  音频诊断 : %CD%\target\release\audio-probe.exe
echo  桌面应用 : %CD%\target\release\bundle\nsis\*.exe
echo  裸 exe   : %CD%\target\release\moonshine-meeting-assistant.exe
echo ==========================================================
echo.
echo 建议先运行 audio-probe.exe（或 audio-probe.exe --level）确认内录设备可用，
echo 再启动桌面应用；云 ASR 引擎不需要 Python，本地引擎需要 pip 依赖。
echo.
pause
