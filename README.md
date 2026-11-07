# SilhouetteTetris

**Windows 11 · Rust · egui · Cold Clear 2 · Tetris Bot Protocol (TBP)**

PNG 템플릿이나 테트로미노별 고정 RGB 색상표 없이, 화면의 격자·명암 대비·실루엣·시간적 움직임으로 테트리스 게임 상태를 복원하고 **Cold Clear 2**가 추천한 착수를 실행하는 실험용 비전 봇입니다.

> **프로젝트 상태:** 포함된 Local Lab과 절차적 테스트 화면에서 검증한 초기 구현입니다. 모든 테트리스 게임/스킨과 초고속 플레이를 보장하는 완성형 범용 봇은 아닙니다. 실측치는 [VALIDATION.md](VALIDATION.md), 이전 T스핀 수정 내역은 [FIX-0.1.1.md](FIX-0.1.1.md)에 있습니다.

## 주요 기능

- **자동 판 탐지:** 10×20 격자 후보를 화면에서 찾고 안정적으로 추적합니다. 판의 위치를 직접 입력할 필요가 없습니다.
- **PNG 없는 형태 인식:** Locked/Active/Ghost/Uncertain을 구분하고 NEXT/HOLD 실루엣을 분석합니다. **밝은 회색/흰색 테두리+어두운 가운데** 형태의 고스트도 인식하며, 실제 잠긴 블록으로 취급하지 않습니다.
- **Cold Clear 2:** 포함된 실행 파일과 TBP(JSON Lines)로 통신합니다.
- **SRS 이동 계획:** 회전·wall kick·중력·잠금 지연을 고려한 키 입력 경로를 탐색합니다. 가능한 일반 착수는 **이동·회전→즉시 하드드롭**을 우선하고, T스핀/끼워 넣기처럼 필요한 상황에서만 소프트드롭을 사용합니다.
- **착수 전후 검증:** 잘못 읽은 상태에서 무조건 하드드롭하지 않습니다.
- **T스핀 경로 복구:** 잠깐 인식이 불명확한 프레임을 재확인하고, 입력 후 화면 캡처·검증에 걸리는 시간(`feedback_ms`)도 중력·잠금 시간 예산에 반영합니다. 하드드롭 **이전**에만 보드·큐·HOLD 동일성을 검증해 최대 3회 안전하게 자동 재계획합니다. 하드드롭이 전송되었거나 실제 상태가 달라졌다면 멈춥니다.
- **egui 디버거:** 원본/흑백/에지/격자 후보/복원 보드와 입력·엔진 상태를 표시합니다.
- **Always on Top:** 디버거 창이 기본적으로 항상 위에 표시됩니다. GUI의 **Always on Top** 체크박스로 즉시 끌 수 있습니다.
- **External Tetris Game Support:** 기본적으로 외부 게임 창 입력을 허용합니다. egui에서 **Allow external Tetris games**, **Game Keyboard Bindings** 및 **Apply Game Profile**을 지원합니다.

## 다운로드 후 실행

Windows x64 환경에서 저장소를 내려받은 뒤 루트 폴더에서 다음 중 하나를 실행합니다.

1. **`Start egui.cmd`** — 비전 디버거/컨트롤 창
2. **`Start Local Lab.cmd`** — 테스트용 테트리스 게임

또는 PowerShell에서 직접 실행:

```powershell
.\SilhouetteTetris.exe --gui
.\SilhouetteTetris.exe --test-game
```

프로그램이 처음 켜져도 자동 키 입력은 **비활성**입니다. 별도로 **Start AI**를 눌러야 합니다.

> GitHub의 `SilhouetteTetris.exe`는 커밋 시점의 바이너리입니다. 소스 변경 사항은 새 바이너리를 빌드해야 적용됩니다. 이 README의 새 복구·Always on Top 수정은 제공된 기존 EXE에 자동 반영되지 않습니다.

## 외부 테트리스 게임 연결 (예: Cold Clear)

기존 `default compatibility profile permits only Local Lab; target is "Cold Clear (FPS: 59)"` 오류의 원인이었던 외부 창 차단은 기본 설정에서 해제했습니다. **이제 Local Lab이라는 창 제목을 강제하지 않습니다.**

1. 게임을 실행한 뒤 게임의 키 설정에서 왼쪽/오른쪽/양방향 회전/소프트드롭/하드드롭/홀드 키를 확인합니다.
2. `Start egui.cmd`를 열고 **Allow external Tetris games** 체크 여부를 확인합니다(기본 ON).
3. **Game Keyboard Bindings**를 펼쳐 게임에서 실제 사용하는 키와 일치시키고 **Apply Game Profile**을 클릭합니다. 적용하면 입력 워커가 재시작되며 안전하게 AI가 꺼지고 기준 상태 확인이 초기화됩니다.
4. **Auto Detect Board**로 판을 자동 인식시키고, **Reconstruction**에서 잠긴 셀/액티브/고스트/NEXT/HOLD가 실제 화면과 일치하는지 확인합니다.
5. 게임을 새로 시작했다면 **Confirmed fresh guideline game**을 체크하고, 실제 HOLD가 비어 있을 때에만 **HOLD known empty**를 체크합니다.
6. **Start AI**를 누른 후 게임 창을 클릭해 활성화합니다. 디버거는 Always on Top이어도 게임 창을 덮지 않게 배치하세요. 중단하려면 **F10**을 누르세요.

**핵심 차이:** 외부 창 입력 권한은 모든 게임에서 실제로 판을 인식하거나 키를 자동으로 알아낸다는 뜻이 아닙니다. 이 버전의 비전 파이프라인은 여전히 표준 10×20/SRS, 오른쪽 세로 NEXT·왼쪽 HOLD, 확정된 현재 블록과 숨은 행 기준 상태를 요구합니다. 비표준 회전 규칙, 3D UI, 비어 있거나 가려진 HUD 등에서는 안전한 검증 실패가 나올 수 있습니다.

### 키 설정 파일

기본값은 `config/default.toml`에 있습니다:

```toml
allow_external = true

[controls]
left = "Left"
right = "Right"
cw = "X"
ccw = "Z"
down = "Down"
hard_drop = "Space"
hold = "C"
```

`Up`, `LeftShift`, `Ctrl`, `Space`, `A`~`Z`, 숫자, 방향키, 일부 특수 키를 지원합니다. F10은 긴급 정지로 예약되어 있으며 중복 키 매핑은 거부합니다. GUI에서 설정을 저장하면 `config/local.toml`이 생성됩니다. 다음 실행부터 해당 설정을 쓰려면 `SilhouetteTetris.exe --gui --config config/local.toml`을 실행하세요. 설정을 바꾸고 **Apply Game Profile**을 누르기 전에는 Start AI가 비활성입니다.

## Local Lab에서 자동 플레이하기

1. 두 창을 실행하고 게임 보드가 디버거 창에 가려지지 않도록 배치합니다. Always on Top이 창을 가리면 해제하거나 위치를 옮기세요.
2. **Auto Detect Board**로 판을 찾습니다. 상태가 `LOCKED`가 되고 10×20 복원 화면, 현재 조각, NEXT/HOLD가 맞는지 확인합니다.
3. Local Lab에서 **Reset game**을 누른 직후 **Confirmed fresh guideline game**을 체크합니다. 이 확인은 숨겨진 행이 비어 있다는 사용자 제공 기준 상태입니다.
4. Local Lab 기본 HOLD는 **I**입니다. **HOLD known empty**는 체크하지 마세요.
5. **Start AI**를 누른 뒤 **Local Lab 게임 창을 클릭하여 키보드 포커스를 줍니다.** 디버거는 Always on Top이어도 게임에 키 입력 포커스를 강제로 빼앗지 않습니다.
6. **F10**은 긴급 정지입니다. GUI의 **Pause / Stop / Emergency Stop**도 입력을 멈춥니다.

기본 설정에서 외부 테트리스 창 입력은 활성화되어 있으나, 실제 게임의 키 매핑·보드 규칙·HUD 인식은 확인해야 합니다. 이용이 허용된 환경에서만 실행하세요.

## 조작법과 화면

| 동작 | 키 |
| --- | --- |
| 왼쪽 / 오른쪽 | ← / → |
| 소프트 드롭 | ↓ |
| 시계 / 반시계 회전 | X / Z |
| 하드 드롭 | Space |
| HOLD | C |
| 긴급 정지 | F10 |

위 키는 기본값입니다. 실제 키 매핑은 GUI의 **Game Keyboard Bindings** 또는 설정 파일에서 바꿀 수 있습니다. 게임의 키가 다르면 반드시 수정하세요.

GUI의 `Original`, `Grayscale`, `Edges`, `Heatmap`, `Occupancy`, `Silhouettes`, `Reconstruction` 보기로 인식 단계별 문제를 확인할 수 있습니다. **Always on Top**은 디버거 창의 순서만 변경하며 게임 창의 입력 포커스를 자동 변경하지 않습니다.

## 회색 고스트 블록(Ghost Piece)은 착수 목표가 아닙니다

테트리스 게임의 회색/흰색 실루엣은 **현재 블록의 예상 자연 낙하 위치**일 뿐입니다. 이는 실제로 쌓인 블록도 아니고 Cold Clear 2가 지정한 목표도 아닙니다.

- 판에는 **실제 잠긴 셀만** 전달합니다. 고스트가 빈칸에 표시돼도 충돌 장애물로 처리하지 않습니다.
- 중앙이 어둡고 주변이 밝은 고스트/얇은 윤곽선 고스트를 형태로 식별한 뒤, 현재 블록을 보드 위로 떨어뜨려 계산한 착지 위치와 일치할 때만 Ghost로 표시합니다.
- 고스트 칸이 다른 위치에 표시된다고 AI가 그 위치로 블록을 움직이지 않습니다. **AI 추천과 경로 검증이 통과하면 곧바로 하드드롭**합니다.
- 고스트로 보이지만 현재 조각의 착지와 일치하지 않는 경우는 지워버리지 않고 불확실한 셀로 처리해 잘못된 착수를 방지합니다.

## T스핀 도중 멈추거나 자동 플레이가 중단될 때

현재 코드는 단발성 격자 점수 저하로 판 추적 기준 상태를 즉시 폐기하지 않고, 입력 직후 잠깐 불명확한 실루엣을 다시 확인합니다. `config/default.toml`의 `movement.feedback_ms`는 각 키 입력 후 실제 화면을 검사하는 데 걸리는 추정 시간(기본 30ms)이며, 하드드롭 전 위치가 어긋나는 문제를 줄이기 위해 이동 계획의 시간 계산에 포함됩니다.

- **하드드롭 전:** 잠긴 보드·현재 조각 종류와 NEXT·HOLD가 동일하다고 다시 확인되면 현재 위치에서 경로를 재계획합니다. **최대 3회**까지만 시도합니다.
- **하드드롭 이후 / 예상치 못한 잠금:** 자동 재시작하지 않습니다. 화면 상태가 바뀐 상태에서 재입력하면 잘못된 블록을 잠글 수 있기 때문입니다.
- **창 포커스 상실:** 게임 창을 다시 활성화하세요. 디버거 창이 보드를 가리면 위치를 바꾸거나 Always on Top을 끄세요.
- **실패 원인:** 실행 폴더의 `logs/last-error.json`, `logs/last-recovery.json`, `logs/headless-report.json` 및 GUI의 **Last verification error**를 확인하세요.

실제 게임에서 같은 문제가 계속 나면 에러 로그와 재현 상황을 확인해야 합니다. 코드상 복구 경로를 추가한 것이 모든 T스핀의 성공을 보장하지는 않습니다.

## 소스 빌드

Rust stable x64 MSVC와 Visual Studio C++ Build Tools, Windows SDK가 필요합니다.

```powershell
cargo fmt --all --check
cargo test --locked
cargo build --release --locked
cargo run -p app --release --bin silhouette-tetris -- --gui
```

소스 빌드 결과는 **`target/release/silhouette-tetris.exe`** (`crates/app/Cargo.toml`의 `[[bin]]` 설정)입니다. 저장소 루트에 이미 포함된 `SilhouetteTetris.exe`와는 **다른 파일**이므로, 새 코드를 실행할 때는 빌드 결과를 직접 실행하거나 루트 EXE를 별도로 교체해야 합니다.

Cold Clear 2 엔진은 `engines/cold-clear-2.exe`에 포함되며 소스는 `third_party/cold-clear-2/`에 보관됩니다. 엔진을 다시 빌드하려면 `./scripts/build-engine.ps1`을 사용합니다.

읽기 전용 Headless 실행:

```powershell
.\SilhouetteTetris.exe --headless --seconds 15
```

포커스가 맞는 게임 창에서만 실제 입력을 활성화하는 headless 테스트(새 게임 상태를 명시적으로 확인한 경우에만):

```powershell
.\SilhouetteTetris.exe --headless --allow-input --fresh-game --seconds 20
```

## 구성

| 경로 | 역할 |
| --- | --- |
| `crates/capture` | Windows 캡처, ROI |
| `crates/detection` | 보드 후보 검출·추적 |
| `crates/vision` | 셀 점유·실루엣·HUD 인식 |
| `crates/game-state` | 테트리스 상태와 TBP 좌표 |
| `crates/tbp-client` | Cold Clear 2 TBP 클라이언트 |
| `crates/movement` | 이동/회전 경로 탐색 |
| `crates/input` | 키 입력, 포커스, 취소 |
| `crates/gui` | egui 디버거·Local Lab·실행 파이프라인 |
| `crates/diagnostics` | 지연과 실행 통계 |
| `config/default.toml` | 기본 설정 |
| `tests/` · `evidence/` | 회귀 테스트와 기존 검증 근거 |

## 성능과 제한

[VALIDATION.md](VALIDATION.md)의 **기존 버전** 15초 Windows Local Lab 실측은 화면 검증 착수 22회, 약 **1.467 PPS**, 캡처 평균 **29.73ms**였습니다. 이 수치는 이번 수정 후 재측정 값이 아닙니다. 10+ PPS나 120FPS 목표를 달성했다고 주장하지 않습니다.

캡처는 현재 GDI 기반이며 DXGI는 미구현입니다. 180도 회전, 게임별 DAS/ARR·비표준 SRS·다양한 HUD 스킨과 3D/격자 없는 보드는 추가 개발 및 실게임 검증이 필요합니다. 인식 신뢰도는 정답 데이터셋 기준 정확도와 다릅니다.

## 라이선스 / 원본

- 프로젝트: [MIT License](LICENSE)
- Cold Clear 2: [원본 저장소](https://github.com/MinusKelvin/cold-clear-2), MIT 또는 Apache-2.0
- TBP: [tetris-bot-protocol/tbp-spec](https://github.com/tetris-bot-protocol/tbp-spec)
- 3rd-party 고지: [THIRD_PARTY.md](THIRD_PARTY.md)
