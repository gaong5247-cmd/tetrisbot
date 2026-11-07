# SilhouetteTetris

**Rust · Windows 11 x64 · egui · Cold Clear 2 · Tetris Bot Protocol**

게임 화면에서 **테트리스판, 쌓인 블록, 현재 떨어지는 테트로미노만** 인식해 Cold Clear 2가 추천한 배치를 실행하는 실험용 비전 AI입니다.

> **Current-Piece-Only:** NEXT/HOLD 패널을 탐색하거나 분류하지 않습니다. 다음 블록 순서나 실제 홀드 내용은 엔진 입력에도 사용하지 않고, 봇은 **HOLD 키를 누르지 않습니다.** 현재 블록의 모양, 현재 위치 및 잠긴 보드 상태만 매 턴 사용합니다.

![No template required](https://img.shields.io/badge/PNG_templates-not_required-informational)
![Rust](https://img.shields.io/badge/Rust-2024-orange)
![Protocol](https://img.shields.io/badge/Engine-Cold_Clear_2-blue)

## 기능

- **판 자동 탐지:** 화면에서 10열 × 20행 테트리스판과 셀 간격을 찾고 추적
- **색상표·PNG 불필요:** 테트로미노 실루엣, 셀 경계, 명암·시간 변화로 현재 상태 추정
- **고스트 피스 제외:** 현재 블록의 예상 착지 실루엣을 잠긴 블록과 구별
- **현재 블록만 탐색:** Cold Clear 2 / TBP로 현재 조각의 착수 후보를 구하고 실제로 도달 가능한 경로 선택
- **HOLD 금지:** 이동 경로 및 키 입력기에서 HOLD 동작을 거부
- **빠른 하드드롭:** 단순 배치에서는 좌우 이동·회전 후 하드드롭을 우선, T스핀 등은 정밀 이동
- **실시간 egui:** 실제 캡처, 격자, 현재 조각, 잠긴 보드, 엔진 추천 위치, 오류 및 측정치
- **Always on Top:** 디버거 창을 위에 고정하거나 체크박스로 해제 가능
- **안전한 재동기화:** 매 착수마다 실제 화면에서 보드를 다시 복원하고 Cold Clear 2를 새 상태로 시작

**중요한 제한:** NEXT가 없으면 미래 조각을 내다보는 탐색은 줄어들고, 실제 HOLD를 쓸 수 없으므로 완전한 큐 기반 플레이보다 전략적 성능이 낮아질 수 있습니다. 모든 테트리스 게임과 스킨, 극한 PPS가 검증된 것은 아닙니다. 기본 구현은 표준 10×20/SRS 화면을 대상으로 합니다.

## 실행 (Windows)

[GitHub Actions](https://github.com/gaong5247-cmd/tetrisbot/actions)에서 성공한 **Windows Rust build and regression tests** 실행을 열고 페이지 하단의 **SilhouetteTetris-Windows-x64** 아티팩트를 받습니다. ZIP을 모두 압축 해제한 다음:

- `Start egui.cmd`: 화면 인식 및 자동 플레이 디버거
- `Start Local Lab.cmd`: 포함된 테스트용 테트리스 게임

또는 같은 폴더에서:

```powershell
.\SilhouetteTetris.exe --gui
.\SilhouetteTetris.exe --test-game
```

> 저장소 루트의 기존 `SilhouetteTetris.exe`는 예전 버전일 수 있습니다. 새 코드가 반영된 실행 파일을 사용하려면 성공한 Actions 아티팩트를 받거나 직접 다시 빌드해야 합니다.

### 자동 플레이 순서

1. 테트리스 게임을 켠 뒤 egui 디버거를 실행합니다.
2. **Auto Detect Board**로 보드를 찾습니다. `LOCKED`가 되고 **Reconstructed Board**의 고정 셀과 **Current piece**가 실제 화면과 일치하는지 확인합니다.
3. 새 게임을 시작했다면 **Confirmed fresh guideline game**을 체크합니다. 보이지 않는 내부 행이 비어 있다는 사실을 사용자가 확인하는 과정입니다.
4. 게임의 키 조작이 다르면 **Game Keyboard Bindings**에서 좌우 이동, 회전, 소프트드롭, 하드드롭을 맞춘 다음 **Apply Game Profile**을 누릅니다. (HOLD는 설정 대상이 아닙니다.)
5. **Start AI**를 누르고 실제 게임 창을 클릭해 키보드 포커스를 줍니다. `Always on Top` 때문에 게임 화면이 가려지면 해제하거나 디버거를 옮깁니다.
6. 언제든지 **F10** 또는 GUI의 **Pause / Stop / Emergency Stop**으로 취소할 수 있습니다.

기본적으로 외부 테트리스 창도 허용하지만, **게임 규칙·회전 방식·키 매핑이 맞지 않으면 입력하지 않을 수 있습니다.** 온라인 게임에서 허용되는 자동화인지 해당 서비스의 규정을 확인하세요.

## Current-Piece-Only 모드 동작

```text
화면 캡처 → 보드 자동 탐지 → 실제 잠긴 셀 + 현재 블록 형태
    → Cold Clear 2 (현재 조각 1개, 미래 큐 없음)
    → 현재 조각으로 가능한 추천 수만 필터링
    → 이동/회전 경로 계산 → 최종 착지 검증 → Hard Drop
    → 잠긴 보드 + 새로 나타난 현재 조각 재검증 → 반복
```

- **NEXT 패널:** 픽셀을 읽거나 순서를 예측하지 않습니다.
- **HOLD 패널:** 비어 있는지 판단하지 않습니다. HOLD 버튼을 보내지 않습니다.
- **Cold Clear 2 내부 가상 reserve:** 엔진의 자료구조 때문에 현재 블록과 같은 조각을 임시 reserve로 전달하지만 **실제 게임의 홀드 상태를 나타내지 않습니다.** 보드가 갱신되면 엔진 상태를 초기화합니다.
- **비전 실패:** 현재 블록을 알아보지 못하거나 확정된 보드가 바뀐 경우 키 입력을 멈춥니다. 안전 검증을 끄지 않습니다.

그레이스케일이나 고스트 피스가 보이는 게임에서도 **PNG 템플릿이나 고정 색상 테이블이 필요하지 않습니다.** 그렇다고 모든 3D·무테두리·이펙트 많은 게임을 확실히 인식한다는 뜻은 아닙니다.

## 기본 키와 설정

`config/default.toml`에서 조작과 탐지 설정을 관리합니다. 대표 기본값은 아래와 같습니다.

| 동작 | 기본 키 |
|---|---|
| 왼쪽 이동 | Left |
| 오른쪽 이동 | Right |
| 시계 방향 회전 | X |
| 반시계 방향 회전 | Z |
| 소프트 드롭 | Down |
| 하드 드롭 | Space |
| 긴급 정지 | F10 |
| HOLD | **사용하지 않음** |

키는 egui에서 수정한 뒤 **Apply Game Profile**을 누르세요. **Save Configuration**은 `config/local.toml`에 저장하므로, 다음 실행에 사용하려면 `--config config/local.toml`을 지정합니다.

## 개발·테스트

Rust stable (MSVC x64), C++ Build Tools, Windows SDK가 필요합니다.

```powershell
cargo test -p app --locked
cargo build -p app --release --locked
cargo run -p app --release --bin silhouette-tetris -- --gui
```

소스 빌드 산출물은 `target/release/silhouette-tetris.exe`입니다. 실행 시 `engines/cold-clear-2.exe`와 상대 경로 설정을 찾을 수 있도록 프로젝트 루트에서 실행하세요.

실제 Cold Clear 2 프로세스 연동 및 절차적 화면 기반 연속 착수 테스트는 Windows CI에 포함됩니다. 사용자의 외부 게임에서의 장시간 자동 플레이/PPS 성공을 대신 증명하지는 않습니다.

| 경로 | 역할 |
|---|---|
| `crates/capture` / `crates/detection` | Windows 화면 캡처 / 보드 자동 탐지 |
| `crates/vision` | 현재 블록·잠긴 보드·고스트 구분 (**HUD 감지 없음**) |
| `crates/game-state` | 관측 보드와 현재 블록으로 CC2 상태 구성 |
| `crates/tbp-client` | Cold Clear 2 TBP 통신 |
| `crates/movement` / `crates/input` | 실제 이동 경로 / 안전한 입력 |
| `crates/gui` | egui 디버거, Local Lab, 런타임 |
| `tests` / `evidence` | 회귀 테스트와 과거 실측 자료 |

과거 측정 수치는 [VALIDATION.md](VALIDATION.md)를 참고하되, **이 모드의 속도 측정으로 해석하면 안 됩니다.** [THIRD_PARTY.md](THIRD_PARTY.md)에 Cold Clear 2 원본 라이선스 안내가 있습니다.

## 라이선스

프로젝트는 [MIT](LICENSE)입니다. 번들된 [Cold Clear 2](https://github.com/MinusKelvin/cold-clear-2)는 MIT 또는 Apache-2.0 라이선스를 따릅니다.
