@echo off
cd /d "%~dp0"
if exist "SilhouetteTetris.exe" (
  "SilhouetteTetris.exe" --test-game
) else (
  cargo run --release -- --test-game
)
