@echo off
chcp 65001 >nul
setlocal
echo ============================================================
echo   Moonshine 会议助手 - 启动桌面界面（开发模式）
echo ============================================================
echo.
echo 说明：本脚本直接编译并运行桌面应用，会弹出带三栏界面的窗口，
echo       不打包安装程序，比 build-windows.bat 快得多（首次需编译依赖）。
echo.

where cargo >nul 2>&1 || (
  echo [错误] 未找到 cargo，请先安装 Rust：https://rustup.rs
  pause & exit /b 1
)

echo 正在编译并启动（首次约 5-15 分钟，之后增量编译很快）...
echo.
cargo run -p moonshine-meeting-assistant

if errorlevel 1 (
  echo.
  echo [启动失败] 常见原因：
  echo   1. 缺少 Visual Studio Build Tools（勾选「使用 C++ 的桌面开发」）
  echo   2. 缺少 WebView2 运行时（Win11 自带；Win10 从微软官网安装 Evergreen Runtime）
  echo   3. 端口/权限问题：尝试以管理员身份运行
  echo.
  pause
)
