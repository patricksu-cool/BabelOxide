@echo off
setlocal EnableExtensions DisableDelayedExpansion
set "PYTHONUTF8=1"
"%SystemRoot%\System32\chcp.com" 65001 >nul
where uv >nul 2>nul
if errorlevel 1 (
    echo [ERROR] uv is required. Install uv, then run this launcher again. 1>&2
    set "BABEL_DEV_EXIT=1"
    goto finish
)
uv run --locked --no-dev --python 3.13.16 --project "%~dp0." python -B "%~dp0script\dev.py" %*
set "BABEL_DEV_EXIT=%ERRORLEVEL%"
:finish
if not "%BABEL_DEV_EXIT%"=="0" echo Development launch failed. Exit code: %BABEL_DEV_EXIT% 1>&2
if "%~1"=="" pause
exit /b %BABEL_DEV_EXIT%
