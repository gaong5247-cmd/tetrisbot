# SilhouetteTetris

Windows 11 x64용 Rust/egui 화면 인식 테트리스 실험 프로젝트입니다. PNG 템플릿이나 RGB→조각 ID 표를 사용하지 않습니다. 자동 격자 탐지, 셀 특징과 시간 변화에 의한 상태 복원, 실제 Cold Clear 2 TBP 자식 프로세스, SRS 경로 탐색, SendInput 및 착수 후 검증을 포함합니다.

**현재 검증 범위는 포함된 Local Lab과 절차적 테스트 장면입니다. 모든 게임에서 작동하는 완성된 범용 인식기로 간주하면 안 됩니다.** 세부 측정과 미검증 항목은 [VALIDATION.md](VALIDATION.md)에 기록합니다.

## 바로 실행

`Start egui.cmd`를 실행하거나 이 폴더에서:

```powershell
.\SilhouetteTetris.exe --gui
```

소스 빌드에는 Rust stable x64 MSVC와 Visual Studio C++ Build Tools/Windows SDK가 필요합니다. 현재 검증한 툴체인은 rustc 1.98.1입니다.

```powershell
cargo build --release --locked
cargo test --locked
cargo run --release -- --gui
```

Cold Clear 2 실행 파일은 `engines/cold-clear-2.exe`에 포함되어 있습니다. 소스에서 다시 빌드하려면:

```powershell
.\scripts\build-engine.ps1
```

엔진 소스와 라이선스는 `third_party/cold-clear-2/`에 있습니다. 네트워크 없이 소스를 보관하되 첫 Cargo 빌드에는 crates.io 의존성을 내려받아야 합니다.

## 로컬 게임으로 확인

1. `Start Local Lab.cmd` 또는 `cargo run --release -- --test-game`으로 게임 창을 엽니다.
2. 디버거와 게임 창이 겹치지 않게 둡니다. 보드 좌표를 지정할 필요는 없습니다. 디버거 창의 영역은 캡처 버퍼에서 가려 자기 화면을 다시 탐지하지 않게 합니다.
3. 디버거가 `SEARCHING → CANDIDATE → VALIDATING → LOCKED`로 바뀌고, 보드 좌표·셀 크기·현재 조각·NEXT·HOLD를 표시하는지 확인합니다.
4. 게임을 Reset한 직후 `Confirmed fresh guideline game`을 체크합니다. 이것은 화면에서 보이지 않는 20개 행이 비어 있고 combo=0/B2B=false라는 **사용자 제공 기준 상태**입니다. 앱이 숨은 행을 추측한 것이 아닙니다.
5. 기본 Lab HOLD는 I가 들어 있습니다. `HOLD known empty`는 체크하지 않습니다. 해당 체크는 실제 빈 HOLD를 사용자가 확인한 경우에만 사용합니다.
6. 기준 상태가 있으면 엔진 추천이 노란 외곽선으로 표시됩니다. 이때도 키 입력은 비활성입니다.
7. `Start AI`를 별도로 누른 뒤 게임 창에 포커스를 줍니다. 입력은 포커스가 있는 탐지 대상 창으로만 전송됩니다.
8. `F10`은 전역 긴급 정지입니다. Pause/Stop/Emergency Stop도 입력을 취소하며, 보유 중인 키를 해제합니다.

기본 입력 프로필은 Local Lab만 허용합니다. 외부 게임은 `allow_external = true`와 해당 게임에 대해 검증한 이동 프로필이 필요합니다. 이 옵션 자체가 규칙 호환성을 검증해 주지는 않습니다. 로컬·허용된 환경에서만 사용하십시오.

## GUI

왼쪽에는 실제 캡처와 후보/NMS 결과, 탐지 격자·중심점·신뢰도·데스크톱 좌표를 표시합니다. 중앙에는 잠긴 셀, 액티브, 고스트, 불확실한 셀, 엔진 추천을 표시합니다. 오른쪽에는 상태·통신·평균/p95 지연·검증 실패 이유가 나옵니다.

`Start egui`는 실시간 프리뷰 진입점입니다. `Auto Detect Board`/`Rescan`은 좌표를 버리고 다시 탐색합니다. `Start Vision`, `Start AI`, `Pause`, `Stop`, `Emergency Stop`, `Save Configuration`, `Debug Mode`를 제공합니다. 저장된 설정은 `--config config/local.toml`로 불러옵니다. 세션의 입력 활성 상태와 숨은 행 확인은 설정 파일에 저장하지 않습니다.

캡처 뷰는 Original/Grayscale/Edges/Heatmap을 제공하고 Occupancy/Silhouettes/Reconstruction으로 내부 상태를 확인할 수 있습니다. Heatmap은 후보 점수 오버레이이며 학습 모델의 픽셀 확률 지도가 아닙니다. 셀 confidence는 경험적 특징 점수이며 통계적으로 보정된 인식 정확도가 아닙니다.

## Headless

기본 headless는 읽기 전용입니다.

```powershell
.\SilhouetteTetris.exe --headless --seconds 15
```

실제 입력은 두 플래그를 모두 명시하고, 새 Local Lab을 포그라운드에 둔 경우에만 허용합니다.

```powershell
.\SilhouetteTetris.exe --headless --allow-input --fresh-game --seconds 20
```

결과는 표준 출력과 `logs/headless-report.json`에 저장됩니다. 검증 실패가 있으면 종료 코드가 실패가 됩니다. `--fresh-game`은 숨은 행/체인 상태에 관한 명시적 확인입니다. 중간 게임 상태에 사용하면 안 됩니다.

## 구현 구조

| Crate | 역할 |
|---|---|
| capture | 모니터 열거, 물리 픽셀 GDI 캡처, ROI, 디버거 마스킹 |
| detection | 에지 run으로 격자 가설 생성, 반복 경계·4셀 증거 평가, NMS, 시간적 추적 |
| vision | 배경의 적응적 색 분포와 대비/텍스처/윤곽/시간 차분, 7종 형태 매칭, HUD 실루엣 |
| game-state | Empty/Locked/Active/Ghost/Uncertain, 관측 가능성, bottom-up 10×40 변환 |
| tbp-client | 실제 엔진 프로세스, NDJSON, handshake, timeout, play/new_piece/resync |
| movement | SRS true-rotation, I/O/JLSTZ kick, 충돌·중력·잠금 예산을 포함한 pulse BFS |
| input | Windows SendInput, 키 해제 RAII, 전경 창 바인딩, F10 취소 |
| gui | egui 디버거, 독립 Local Lab, 워커 연결과 검증 트랜잭션 |
| diagnostics | 평균·p95·최대 지연의 제한된 표본 창 |
| app | GUI/headless/test-game CLI |

캡처와 인식 사이, 전체 검색과 인식 사이에는 bounded latest-frame 채널을 사용합니다. 오래된 프레임은 버립니다. 전체 스캔은 별도 스레드이며 정상 추적 중에는 보드와 HUD 주변 ROI만 캡처합니다. 추적 실패 시 ROI 주변을 먼저 검색하고 전체 모니터로 확장합니다. 엔진은 프레임마다 시작하지 않습니다. 게임 상태 이벤트에 따라 추천하며, 검증된 착수에만 `play`와 실제 관측한 새 큐 조각으로 `new_piece`를 보냅니다.

## TBP 검증 근거

Cold Clear 2 revision `ed8b19327b6bd1410ddd873d8611485bd45d8fae`의 `main.rs`, `lib.rs`, `tbp.rs`, `data.rs`, `movegen.rs`와 [TBP MVP 명세](https://github.com/tetris-bot-protocol/tbp-spec/blob/main/text/0000-mvp.md)를 확인했습니다.

`start.board`는 **맨 아래 행부터** 40×10이며 `queue`는 **현재 액티브 + 관측한 NEXT**입니다. 빈 HOLD를 처리하는 엔진 내부 reserve 규칙 때문에 현재 액티브를 임의로 빼지 않습니다. 추천은 `location: {type, orientation, x, y}`와 `spin: none|mini|full`입니다. I/O를 포함한 좌표는 좌상단이 아닌 SRS true-rotation 기준입니다. HOLD는 추천 조각과 액티브/HOLD/NEXT를 비교해 판정합니다.

실제 엔진은 `start` 직후 아직 탐색하지 못해 빈 추천 배열을 반환할 수 있습니다. 클라이언트는 전체 timeout 내에서만 제한적으로 다시 요청합니다. timeout/EOF/프로세스 종료는 성공으로 처리하지 않습니다.

## 안전한 상태 전이

LOCKED 전에는 입력을 허용하지 않습니다. 셀·액티브·NEXT·HOLD·숨은 행·게임 규칙 정보가 부족하면 기다립니다. 경로 실행 전 재캡처하고, 하드드롭 직전에는 현재 보드와 액티브의 실제 착지 셀을 다시 검사합니다. 착수 후에는 예상 잠긴 보드, 새 액티브, NEXT shift, HOLD를 연속 두 프레임에서 확인합니다. 불일치 시 stop 후 일시 중단하며, 사용자가 상태를 재확인하고 Start AI로 재개할 수 있습니다.

## 지원 한계 / 남은 작업

- 격자 또는 반복 셀 경계와 대비가 있는 보드 extent가 필요합니다. 격자·테두리·배경 구분이 모두 없는 장면, 가려진 보드, 투명/복잡한 배경, 3D/기울어진 판은 미지원입니다.
- 격자 없는 약한 후보는 실제 조각 이동까지 관측해야 입력 신뢰도를 얻습니다. 표준 10×20을 대상으로 하며 숨은 행을 관측한 것으로 간주하지 않습니다.
- HUD는 주변의 고립된 4셀 형태를 다중 축척으로 찾습니다. 현재 기본 가설은 오른쪽의 수직 NEXT, 왼쪽 HOLD입니다. 가로 큐/중앙 HOLD/효과로 연결된 조각/부분 가림은 확정하지 않습니다.
- 처음부터 스택에 붙어 있거나 가려진 액티브는 이전 프레임의 신뢰할 수 있는 잠긴 보드가 없으면 확정하지 않습니다. 메뉴/게임오버 화면에 별도 OCR 분류기는 없으며, 유효 상태가 사라지면 입력이 중단됩니다.
- 이동은 한 키를 DAS 이전에 떼는 펄스 방식입니다. ARR 연속 반복을 이용하는 고속 실행, 180도 회전, 게임별 비표준 kick, 무한/제한 lock reset의 정밀 모델은 구현 범위 밖입니다. 중력 위상은 근사하며 하드드롭 전 화면 검사로 보완합니다.
- 캡처는 순수 Windows GDI fallback입니다. DXGI Desktop Duplication, GPU 처리, 전체 버퍼 풀 재사용은 아직 적용하지 않았습니다. 120 FPS/10 PPS를 달성했다고 주장하지 않습니다.
- 임의의 게임에 대한 장기 인식률·PPS와 전체 화면/다중 모니터 해상도 전환 실게임 검증은 추가로 필요합니다. 실패한 실험도 VALIDATION에 남깁니다.

## 테스트와 벤치마크

```powershell
cargo test --locked
cargo bench --bench pipeline
```

테스트 이미지는 Rust 코드로 생성합니다. 런타임 템플릿이 아닙니다. TBP 통합 테스트는 실제 `engines/cold-clear-2.exe`가 없으면 실패하며 가짜 엔진으로 대체하지 않습니다. `CC2_EXE`로 다른 실제 빌드를 지정할 수 있습니다.

이 프로젝트 코드는 MIT, Cold Clear 2는 MIT 또는 Apache-2.0입니다. [THIRD_PARTY.md](THIRD_PARTY.md)를 참고하십시오.
#   t e t r i s b o t  
 