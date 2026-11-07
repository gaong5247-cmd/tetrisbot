@echo off
cd /d "%~dp0"
if exist "SilhouetteTetris.exe" (
  "SilhouetteTetris.exe" --gui
) else (
  cargo run --release -- --gui
)
