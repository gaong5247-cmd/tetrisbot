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

## 흐린 고스트 피스와 멀티플레이 가비지 공격

**색이 연한 실체형 고스트:** 일부 게임은 고스트를 외곽선 대신 현재 조각과 같은 색상의 어두운 블록 4개로 그립니다. 예: 밝은 보라색 T와 그 아래 절반 밝기의 보라색 T. 이번 버전은 인접한 두 조각을 색의 밝기·색조·형태로 분리합니다. 어두운 4칸이 현재 조각의 **물리적으로 계산된 착지 위치와 정확히 일치**할 때만 Ghost로 처리합니다. 색이 어둡다는 이유만으로 실제로 잠긴 블록을 제거하지 않습니다.

**합의된 공격 멀티플레이:** GUI 상단 **Multiplayer Attack (bottom garbage rises)**를 켜고 **Apply Game Profile**을 누르세요. 기본은 OFF이며 `config/default.toml`의 `multiplayer_attack_mode = true`로도 설정할 수 있습니다.

- 아래에서 가비지 줄이 들어와도, 화면 상단의 비어 있는 셀을 기준으로 실제 배경색을 유지하므로 빈칸이 있는 줄을 전체 블록으로 오인하는 것을 줄입니다.
- 가비지 줄은 각 행이 대부분 차 있지만 실제 구멍이 있는지 검사합니다. 기존 보드가 **정확히 N줄 위로 밀려난 모습**과 일치하는 경우에만 공격으로 승인합니다.
- CC2 계산 중 공격이 들어오면 새로 관측된 **현재 보드와 현재 블록**으로 재계획합니다. 이동 중이라면 하드드롭을 억지로 보내지 않고 화면을 다시 확인합니다.
- 내 블록을 놓은 직후 공격이 들어와 예상 스택과 달라진 경우에도, 실제 가비지 N줄이 정확히 들어왔는지 **연속 두 프레임** 확인한 뒤 다음 턴을 계산합니다.
- **완전히 막힌 판, 보이지 않는 상단 행, 예상과 다른 줄 이동, 불분명한 픽셀**은 억지로 빈칸을 만들어 착수하지 않습니다. 보드가 실제로 탑아웃된 경우는 해당 게임의 판정을 따라야 합니다.

멀티플레이 게임마다 가비지 생성 규칙, 블록 색, 화면 효과와 입력 처리가 다르므로 실제 게임에서 추가 검증이 필요합니다. 합의된 테스트 환경에서 사용하세요.

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

## Cold Clear 게임에서 이동·하드드롭이 안 되는 경우

게임이 정상적으로 화면에 보이더라도, 컴퓨터에서 보낸 키보드 이벤트를 해당 게임이 받아들이는지는 별개의 문제입니다.

1. egui의 **Game Keyboard Bindings**를 열고 게임의 실제 Left/Right/Rotate CW/Rotate CCW/Hard Drop/Soft Drop 키와 일치시키세요. Cold Clear의 일부 버전은 방향키·Z/X·Space를 사용하지만, 게임 안에서 조작이 재설정됐으면 그 설정을 따라야 합니다.
2. **Keyboard input → Scan Code**를 선택하고 **Apply Game Profile**을 누르세요. 최신 기본값은 `scan_code`입니다. 게임이 입력을 무시하면 `Virtual Key`로 바꿔 다시 적용해 보세요.
3. **Start AI**를 누른 뒤 반드시 테트리스 게임 창을 클릭해 전경에 두세요. egui가 Always on Top으로 게임 보드를 가리면 이동하거나 Always on Top을 끄세요.
4. 최신 키 지속 시간은 일반 키 42ms, 하드드롭 65ms이며, 게임이 키를 인식하지 못하면 `input delivery` 단계에서 멈추고 메시지에 해당 키를 적습니다. 입력이 확인되지 않았는데 계속 움직이거나 하드드롭하지 않습니다.

외부 게임이 OS의 합성 키 입력을 차단하거나 다른 입력 방식만 지원한다면 이 설정만으로 호환을 보장할 수 없습니다.

## Roblox 테트리스에서 TBP/착수 후 검증 오류가 나는 경우

- **`TBP suggestion deadline/stdout disconnected`**: 실제 Cold Clear 2 엔진이 응답하지 않았다는 의미입니다. 현재 블록 1개만 쓰는 모드에서 불필요한 7-bag 미래 추측 탐색을 끄고, 입력을 보내기 전 엔진을 **최대 1회** 재시작하도록 수정했습니다. 계속 실패하면 게임 조작 문제가 아니라 엔진 실행/성능 로그를 확인하세요.
- **`post-lock board/current-piece verification timed out`**: 하드드롭 전송 이후 게임이 원하는 보드/새 블록 상태로 바뀐 것을 검사하지 못한 경우입니다. 타임아웃을 3.5초로 늘리고, 하드드롭 직전의 실제 블록 실루엣을 추적기에 남겨 새 스폰과 라인 클리어 효과를 구별하도록 했습니다.
- **`board_mismatch_or_hard_drop_not_received`**: 보드가 예상한 모양이 아니거나 하드드롭 키가 게임에 전달되지 않았을 수 있습니다.
- **`next_active_not_visible`**: 하드드롭 이후 새 활성 블록이 화면에 나타나지 않거나 인식기가 찾지 못했습니다.
- **`uncertain_board_or_animation`**: 블록 셀, 투명 효과, 라인 클리어 애니메이션 등으로 판 인식이 불확실합니다.

실패하면 실행 폴더의 **`logs/last-error.json`**에서 `phase`, `configured_controls`, `post_lock.classification`, `board_difference_cells`, `uncertain_cells`을 확인하세요. 오류 텍스트와 함께 이 파일을 제공하면 어떤 단계에서 멈췄는지 더 정확히 확인할 수 있습니다.

Roblox 경험마다 키 설정·프레임레이트·입력 인터페이스·서버 지연이 다를 수 있습니다. 이 수정은 모든 Roblox 경험의 자동 플레이 성공을 보장하지 않으며, 게임이 허용하는 환경에서만 사용해야 합니다.

## 속도 최적화 (현재 블록 전용)

이 빌드는 정답률을 포기하고 키를 무조건 연타하는 방식이 아니라, **중복 캡처·불필요한 기다림을 줄이는 방식**으로 처리량을 높입니다.

- **검증 캡처 축소:** 키 입력 후 화면 검증은 기존 보드+HUD 주변 약 22×22 셀 영역이 아닌 **실제 10×20 보드**만 캡처합니다. 동일한 셀 크기라면 전송할 픽셀 면적이 약 59% 줄지만, 전체 실행 속도가 같은 비율로 빨라진다는 의미는 아닙니다.
- **단순 좌우 이동 묶음:** 같은 방향의 연속 키 입력은 일부 중간 화면 검사를 생략하고, 확인 시에는 누적 이동량을 검사합니다. **하드드롭 직전의 실제 착지 위치 검증은 항상 유지**합니다.
- **T스핀·소프트드롭:** 회전, 소프트드롭 및 스핀 착수 경로에서는 기존처럼 단계별로 확인합니다.
- **Cold Clear 2:** 현재 조각만 분석하는 엔진의 빈 추천 배열 재요청 간격을 줄이고, 중복 `stop` 전송을 피합니다.
- **후속 프레임 검사:** 게임 화면 자체를 다시 캡처하는 시간과 별도로 대기하던 간격을 단축했습니다.

### 속도 조절

egui에서 **Placement Speed & Safety**를 펼친 뒤 **Horizontal screenshot frequency**를 변경하고 **Apply Game Profile**을 누르세요. 적용하면 새 게임 기준 상태를 다시 확인해야 합니다.

| 값 | 동작 | 사용 시점 |
| --- | --- | --- |
| 1 | 모든 가역적 좌우 이동 뒤 화면 확인 | 키 누락·DAS/ARR가 특이한 게임 |
| 2 (기본값) | 연속 좌우 이동의 검사 일부를 통합 | 속도와 신뢰성의 균형 |
| 3 | 더 넓게 검사 통합 | 키 입력과 게임 반응이 안정적인 환경 |

설정 파일의 `[movement]`에서 `horizontal_verify_stride = 2`로도 지정할 수 있습니다. 예전 `config/local.toml`에는 새 항목이 없어도 기본값 2가 적용됩니다.

**주의:** Cold Clear, Roblox 등 외부 게임에서의 실제 PPS·판단 지연 개선량은 사용자 게임 화면에서 직접 측정해야 합니다. 자동화 테스트는 화면 처리와 이동 계획의 정합성을 확인하지만, 모든 게임의 fps/키 반응 시간을 대신 측정하지 않습니다.

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
