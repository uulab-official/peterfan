# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.20.0] — 정확한 RPM 수치로도 팬 속도 설정 (% 슬라이더에 이어)

### Added
- **팬 속도를 정확한 RPM 숫자로 설정** — 지금까지는 %(퍼센트) 슬라이더로만
  팬 속도를 고정할 수 있었는데, Stats 앱처럼 "2400" 같은 정확한 RPM
  숫자를 입력해서 설정하는 것도 가능해짐. SMC 쪽은 원래 duty%로만
  제어하므로(HardwareProvider가 % 기반), 내부적으로는 입력한 RPM을
  "가장 빠른 팬의 100% 기준 RPM" 대비 %로 환산해 기존 hold 로직에
  그대로 태움 — 새로운 하드웨어 경로 없이 UI만 추가.

## [1.19.0] — Top Processes에서 바로 프로세스 종료

### Added
- **Top Processes 목록에서 프로세스 바로 종료** — 각 프로세스 행에
  마우스를 올리면 우측에 작은 "×" 버튼이 나타남. 클릭하면 확인창을 거쳐
  해당 프로세스에 종료 신호(SIGTERM)를 보냄. 관리자 권한을 쓰지 않으므로
  내 소유가 아닌 프로세스는 OS가 알아서 막아줌(일반 `kill` 명령과 동일한
  권한 규칙) — Activity Monitor를 따로 열 필요 없이 "이 프로세스 뭔데
  이렇게 CPU를 많이 먹지?" → 바로 종료까지 한 번에.

## [1.18.0] — 메뉴바 호버 툴팁 + 디스크 I/O 차트 + 그래프 평균/최고값

### Added
- **메뉴바 아이콘에 마우스만 올려도 뜨는 요약 툴팁** — 클릭해서 팝오버를
  열지 않아도, 메뉴바의 PeterFan 아이콘에 마우스를 올리면 OS 기본
  툴팁으로 "CPU 12.3%  ·  Mem 45.2%  ·  52°C  ·  1180 RPM" 같은 요약이
  바로 뜸. 지금 메뉴바에 표시 중인 지표가 뭐든 상관없이 항상 전체 요약을
  보여줌 — iStat Menus류 앱들의 메뉴바 호버 동작과 동일.
- **디스크 읽기/쓰기 속도 그래프** — 지금까지 디스크 I/O는 숫자로만
  보여줬는데(CPU/메모리/온도/네트워크는 다 그래프가 있었음), 이제 같은
  방식의 스파크라인 차트 추가. 활동이 있을 때만 표시.
- **모든 그래프 아래에 평균/최고값 표시** — "avg 23%  ·  peak 67%"처럼,
  현재 보고 있는 구간(2m/1h/1d)의 평균과 최고값을 그래프 밑에 작게 표시.
  지금까지는 그래프를 봐도 "지금 얼마나 높은 편인지" 감이 안 왔는데,
  기준점이 생김.

## [1.17.0] — 차트 호버 툴팁 + 네트워크 IP 표시 + 프로세스 정렬 (iStat 참고)

### Added
- **차트에 마우스를 올리면 값과 시점이 뜨는 호버 툴팁** — CPU/메모리/온도/
  네트워크 스파크라인이 지금까지는 그냥 그림이었는데, iStat Menus처럼
  차트 위에 마우스를 올리면 그 지점의 정확한 값과 "방금 / N분 전 / N시간
  전" 같은 상대 시점이 작은 말풍선으로 뜸. 2m/1h/1d 탭에 따라 샘플
  간격(1초/1분/1시간)을 반영해 시점을 계산.
- **네트워크 섹션에 로컬 IP 표시** — 지금까지는 송수신 속도만 보여줬는데,
  실제로 트래픽이 나가고 있는(또는 주소가 있는) 인터페이스 이름과 로컬
  IP를 함께 표시 (예: "en0 · 192.168.0.12") — iStat Menus의 네트워크
  모듈처럼 "지금 뭘로 연결돼 있는지" 바로 확인 가능. 외부로 나가는 조회는
  없음(공인 IP 아님, 로컬 인터페이스 정보만).
- **Top Processes CPU/메모리 정렬 전환** — 헤더의 작은 "CPU / MEM" 탭으로
  전환. 이전엔 CPU 사용률 고정 정렬만 가능했음.

## [1.16.0] — 팬 제어 즉시 반영 + 상세 창 닫기 버그 + 언어 설정 + 아이콘 캐시

### Fixed
- **"Max"(또는 다른 속도) 버튼을 눌러도 몇 초씩 늦게 반영되던 문제** —
  `peterfand` 데몬이 명령을 받아도 다음 주기적 틱(기본 2초, 200ms 단위로
  나눠 자다가 깨는 구조)까지 실제로 적용하지 않고 있었다. 이제 IPC로
  auto/rules/profile/hold 명령이 들어오면 그 즉시 잠을 깨워 바로
  적용하고, 온도 기반 재평가를 위한 주기적 틱은 그대로 유지. 체감
  지연이 최대 2~3초 → 약 0.2~1초로 줄어듦.
  ⚠️ 이 수정은 `/usr/local/bin/peterfand`(시스템 데몬, 앱 번들과 별도로
  설치되어 루트로 상시 실행 중)에 반영되어야 효과가 있음 — 업데이트 후
  메뉴에서 **"Enable Fan Control (One-Time Setup)…"**을 한 번 더
  눌러(관리자 암호 재입력) 새 데몬 바이너리를 설치·재시작해야 함.
- **상세 창(Detailed Window)의 빨간 닫기 버튼이 아무 반응이 없던 버그** —
  이벤트 루프가 `WindowEvent::CloseRequested`를 아예 처리하지 않고
  있어서, 일반 창인 상세 창의 닫기 버튼을 눌러도 아무 일도 일어나지
  않았다. 이제 눌렀을 때 창을 숨김 처리(다음에 다시 열면 즉시 재사용).
- **팝오버를 열어도 팬 제어가 안 보이던 문제** — 실제로는 존재했지만
  CPU/메모리/저장공간/온도/팬/배터리/네트워크/프로세스/라이선스까지
  다 지나야 나오는 맨 아래에 있어서, 화면이 작거나 스크롤을 안 하면
  안 보였다. 팬 제어 섹션을 팝오버 맨 위(2m/1h/1d 탭 바로 아래)로
  이동 — 열자마자 바로 보임.
- **rebuild할 때마다 Finder에 앱 아이콘이 예전 것(또는 빈 아이콘)으로
  캐시되던 문제** — 같은 경로·같은 번들 ID로 반복 빌드하면 macOS
  LaunchServices가 아이콘을 캐싱해 새 `AppIcon.icns`가 반영되지 않는
  경우가 있었다. 빌드 스크립트가 이제 매번 `lsregister -f`로 해당
  번들의 캐시를 강제 갱신.

### Added
- **언어 설정 (English / 한국어)** — 우클릭 메뉴의 새 "Language"
  서브메뉴에서 선택. 네이티브 메뉴 항목, 팝오버/상세 창의 주요 문구
  (섹션 이름, 팬 제어 버튼, 라이선스 상태 등)가 번역됨. 선택 즉시
  메뉴·창이 다시 그려져 재시작 없이 바로 적용. 기본값은 "System
  Default"로, `$LANG` 환경변수를 읽어 자동으로 한국어/영어를 고름.

## [1.15.0] — 메뉴바 폭 고정 + 팝오버 높이 제한/스크롤 + 별도 상세 창

### Fixed
- **메뉴바 텍스트 폭이 매 틱 바뀌면서 다른 아이콘들이 계속 좌우로 밀리던
  버그** — "9.5%" → "100.0%"처럼 자릿수가 바뀔 때마다 항목 너비가 변해
  메뉴바 전체가 흔들려 보였다. CPU/메모리/온도/팬/네트워크 전부 고정폭
  포맷(우측 정렬 패딩)으로 변경 — 값이 뭐든 문자 수가 일정해서 폭이
  안정적임.
- **팝오버가 화면 아래로 잘리던 문제** — 섹션이 많이 늘어난 지금(CPU~팬
  제어까지), 특히 작은 화면이나 메뉴바 위치에 따라 팝오버 높이가 화면을
  넘어설 수 있었다. 이제 현재 모니터 높이를 기준으로 최대 높이를 계산해
  캡을 씌우고, 그 이상은 팝오버 내부에서 스크롤되도록 처리(잘리거나
  화면 밖으로 밀리는 대신).

### Added
- **코어별 CPU 표시 개선** — 막대 높이를 2배로 키우고, 각 코어를 자기
  부하 수준에 따라 초록/노랑/빨강으로 색칠, 호버 시 "Core N: XX.X%" 툴팁
  추가. 기존엔 색 구분도 없이 작은 회색 막대만 있었음.
- **"Open Detailed Window…"** — 팝오버 하단 버튼 또는 우클릭 메뉴에서
  열 수 있는 별도의 일반 창(제목표시줄 있음, 크기 조절 가능, 포커스를
  잃어도 안 사라짐). 드롭다운 팝오버는 "잠깐 확인용", 이 창은 "띄워두고
  계속 보는 용"으로 역할 분리. 같은 대시보드 콘텐츠를 공유하되, 창 크기는
  사용자가 직접 조절(팝오버처럼 콘텐츠에 맞춰 자동 리사이즈하지 않음).

## [1.14.0] — 자동 업데이트 + 팝오버 애니메이션/여백 정리

### Fixed
- **`peterfan update`이 잘못된 저장소를 조회하던 버그** — `uulab/peterfan`
  (존재하지 않음) 대신 실제 저장소 `uulab-official/peterfan`을 조회하도록
  수정. 이전까지는 항상 실패하거나 엉뚱한 결과를 냈을 것.
- **팝오버가 열릴 때 "뚜둥"거리며 튀던 문제** — scale/opacity 오픈
  애니메이션을 완전히 제거하고 즉시(한 프레임에) 나타나도록 변경. 이전
  릴리스에서 애니메이션 자체는 다듬었지만, 근본적으로 iStat류 팝오버는
  애니메이션 없이 즉시 뜨는 쪽이 더 매끄럽다는 피드백을 반영.
- **메뉴바 아이콘과 팝오버 사이 여백** — 4px 갭을 없애고 메뉴바에 완전히
  붙여서 뜨도록 수정 (네이티브 Control Center/Wi-Fi 드롭다운과 동일한 방식).

### Added — 자동 업데이트
- **`peterfan_platform::updater`** — GitHub 최신 릴리스 조회, 버전 비교
  (문자열이 아닌 숫자 비교라 "1.13.0" > "1.9.6"을 올바르게 판정), macOS용
  다운로드+설치. 실제 저장소 API 응답으로 회귀 테스트 작성.
- **메뉴바 앱이 실행 후 자동으로 업데이트 확인** (첫 실행 설정 다이얼로그와
  겹치지 않도록 4초 지연) — 새 버전이 있을 때만 다이얼로그로 알림
  ("View Release" / "Not Now" / "Update Now"). "Update Now"는 실제로
  다운로드 → 압축 해제 → 앱 종료 후 번들 교체 → 재실행까지 수행(분리된
  헬퍼 스크립트로 실행되어, 교체 대상인 실행 파일 자신이 실행 중인 상태와
  충돌하지 않음).
- **우클릭 메뉴에 "Check for Updates…"** — 수동으로도 언제든 확인 가능,
  최신 상태일 때도 결과를 알려줌(자동 체크는 조용히 넘어가지만 수동 확인은
  응답함).
- **`peterfan update` CLI**도 동일한 공용 모듈 사용하도록 통일.

## [1.13.0] — 그래프 시간 범위 선택 (2분 / 1시간 / 1일)

iStat Menus의 히스토리 그래프 기간 선택 기능. 지금까지는 항상 최근 2분만
볼 수 있었는데, CPU/메모리/온도/네트워크 그래프를 1시간·1일 단위로도 볼 수
있게 됐다.

### Added
- **팝오버 상단에 "2m / 1h / 1d" 탭** — 클릭 한 번으로 모든 차트(CPU/메모리/
  온도/네트워크)의 표시 범위 전환. 메뉴바 아이콘 자체의 스파크라인은 항상
  최근 2분 그대로(그게 "한눈에 보기"의 역할이므로 범위 선택과 무관).
- **`RangedHistory`** — 원시 초당 샘플(2분, 120개) 위에 분당 평균(1시간,
  60개) → 시간당 평균(1일, 24개)을 누적 롤업. 1일치를 초당 86400개 원시
  샘플로 들고 있지 않고도 세 구간을 동시에 유지. 롤업 정확성과 각 구간별
  용량 상한을 유닛 테스트로 고정.
- 네트워크 히스토리를 rx/tx 별도 배열 대신 합산된 단일 시리즈로 단순화
  (차트가 항상 합계만 그렸으므로 JS 쪽 매 틱 합산 계산도 제거).

## [1.12.0] — 팝오버에 상위 프로세스 목록 + 디스크 I/O 속도

iStat Menus/Stats의 시그니처 기능인 "지금 뭐가 CPU 먹고 있나" 뷰를 메뉴바
팝오버에 추가. CLI(`top`)·TUI에는 이미 있던 기능을 메뉴바에도 확장.

### Added
- **"Top Processes" 섹션** — CPU 사용률 기준 상위 5개 프로세스를 이름/CPU%/
  메모리와 함께 표시. `SystemMonitor::processes()`가 이미 매 틱 갱신되고
  있어 추가 비용 없음.
- **Storage 섹션에 읽기/쓰기 속도 추가** — 기존 정적 용량 표시(%) 아래에
  실시간 디스크 I/O(↓읽기/↑쓰기) 표시, 활동이 있을 때만 노출.

## [1.11.1] — 배포 매끄럽게: ad-hoc 코드사이닝

"설치가 매끄럽지 않다"는 방향으로 계속 — 지금까지는 배포 바이너리가 전혀
서명되어 있지 않아, 실제로 다운로드해서 여는 사용자는 macOS가 "손상되어
열 수 없음"(복구 방법이 안 보이는 무서운 에러)을 띄웠을 것이다. Apple
Developer 계정(유료) 없이도 할 수 있는 선에서 최대한 완화했다.

### Added
- **`scripts/bundle-macos.sh`가 `.app`을 ad-hoc 서명** (`codesign --sign -`)
  — 노터라이제이션은 아니지만, Gatekeeper가 "손상됨"(막다른 느낌) 대신
  표준 "개발자를 확인할 수 없음" 프롬프트(우클릭 → 열기, 또는 시스템 설정에서
  "그래도 열기"로 바로 우회 가능)를 보여주도록 바뀐다.
- **release 워크플로우가 개별 CLI 바이너리(`peterfan`/`peterfan-tui`/
  `peterfand`)도 ad-hoc 서명** — 터미널에서 처음 실행할 때도 동일한 완화 적용.
- **스모크 테스트에 서명 검증 추가** — 회귀 시 바로 잡힘. 추가하자마자
  `pipefail` + `grep -q`의 SIGPIPE 오탐 버그를 스스로 잡아냄(수정 완료).
- README 다운로드 안내에서 존재하지 않는 파일명 패턴
  (`aarch64-apple-darwin` — 실제로는 진작에 `universal-apple-darwin`으로
  바뀌어 있었음)을 바로잡고, 서명 관련 안내로 갱신.

## [1.11.0] — 로그인 시 자동 시작 토글 + doctor 오탐 수정

### Fixed
- **`peterfan doctor`의 "launchd loaded" 체크가 항상 거짓으로 나오던 버그** —
  비권한 프로세스가 `launchctl list <system-domain-label>`로 시스템
  LaunchDaemon을 조회하면 원래 못 본다(자신의 launchd 도메인만 보임). 데몬이
  실제로는 잘 돌고 있어도 항상 "loaded: ✗"로 나왔던 것. 이미 계산돼 있던
  실제 IPC reachability 체크를 재사용하도록 교체 — 추측성 명령 대신 진짜
  근거로 판정.

### Added
- **우클릭 메뉴에 "Launch at Login" 체크박스** — 클릭 한 번으로 로그인 시
  자동 시작 켜기/끄기. 데몬 설치와 달리 사용자 LaunchAgent라 **관리자
  비밀번호가 전혀 필요 없어** 즉시 토글됨. 현재 등록 상태를 반영해 시작 시
  체크 표시.
- **`peterfan_platform::login_item`** — CLI의 `login-item` 서브커맨드가 쓰던
  로직(plist 생성, `find_menubar_binary`, launchctl load/unload)을 공용
  모듈로 분리, CLI와 메뉴바가 동일 코드 공유(`daemon_install`과 같은 패턴).

## [1.10.1] — 첫 실행 시 자동으로 설정 묻기 + --mock 안전장치

"다른 프로그램은 설치하면 끝인데 왜 우리는 준비가 안 되어 있냐"는 피드백
반영. 근본 원인: 정식 상용 앱들은 설치 **패키지(.pkg)** 자체에 권한 설정이
포함되어 설치 시점에 한 번 물어보지만, PeterFan은 `.app`을 그냥 복사하는
방식이라 권한 설정이 "메뉴 뒤져서 찾아야 하는" 단계로 밀려나 있었다.

### Added
- **첫 실행 시 자동 설정 안내 다이얼로그** — 앱을 처음 켜면(그리고 아직
  설정 안 했으면 매번), 메뉴를 찾아 들어갈 필요 없이 바로 "PeterFan needs
  one-time permission to control your Mac's fans. Set up now?" 다이얼로그가
  뜬다. "Set Up Now"를 누르면 그 자리에서 바로 진행, "Don't Ask Again"을
  누르면 config에 저장되어 다시 안 물어봄, "Not Now"는 다음 실행 때 다시 물어봄.
  `MenubarConfig.setup_prompt_dismissed`로 영속화.

### Fixed
- **`--mock` 모드에서도 이 다이얼로그가 뜨던 버그** — 하마터면 mock 테스트
  중에 실제 화면에 다이얼로그가 뜨고, 잘못 누르면 실제 특권 설치가 실행될
  뻔했다(테스트 중 실제로 한 번 발생 — 고아 `osascript` 프로세스가 화면에
  남아있는 걸 발견해 즉시 종료). `--mock`일 때는 이 플로우 자체를 건너뛰도록
  수정하고, 스모크 테스트에 "`--mock` 실행 시 `osascript`가 절대 뜨면 안 됨"
  체크를 추가해 재발 방지.

## [1.10.0] — 메뉴바에서 터미널 없이 팬 제어 활성화

팬 속도 제어가 macOS SMC 정책상 root 권한 없이는 절대 불가능하다는 사실은
바뀌지 않지만, 지금까지는 그 권한을 얻으려면 **반드시 터미널을 열고
`peterfan install-daemon`을 직접 입력**해야 했다. GUI 앱을 쓰는 사용자에게
터미널을 요구하는 건 그 자체로 완성도 문제 — 이번 릴리스로 제거했다.

### Added
- **우클릭 메뉴 맨 위에 "Enable Fan Control (One-Time Setup)…"** — 클릭하면
  터미널 없이 바로 그 자리에서 macOS 관리자 비밀번호 창이 뜨고, 완료되면
  데스크탑 알림으로 성공/실패를 알려준다. 이후로는 sudo 없이 메뉴바에서
  팬 속도가 실제로 바뀐다.
- **`peterfan_platform::daemon_install`** — CLI의 `install-daemon`/
  `uninstall-daemon`이 쓰던 로직(바이너리 탐색, LaunchDaemon plist 생성,
  `osascript … with administrator privileges` 실행)을 공용 모듈로 분리해
  CLI와 메뉴바 앱이 동일한 코드로 동작. `InstallOutcome` enum으로 "설치됨" /
  "설치됐지만 응답 없음" / "드라이런"을 명확히 구분(문자열 추측 없음).
- `scripts/bundle-macos.sh`가 이제 `PeterFan.app/Contents/MacOS/`에
  `peterfand`도 함께 담는다 — 메뉴바 앱이 자기 옆에서 찾아 설치할 수 있도록.
  스모크 테스트에 번들 검증 추가(빠뜨리면 "Enable Fan Control"이 조용히
  실패하므로).

### Changed
- 설치 관련 privileged 셸 스크립트 실행은 admin 비밀번호 응답을 기다리는
  동안 블로킹되므로, 메뉴 클릭 시 이벤트 루프가 아닌 백그라운드 스레드에서
  실행 — 대화상자가 떠 있는 동안 메뉴바가 멈추지 않는다.

## [1.9.6] — 테스트 하네스: 스모크 테스트 + 클릭 라우팅 회귀 테스트

지금까지 나온 버그들(스마트따옴표 JS, 좌클릭이 메뉴를 가로챈 것, `hold:`
미처리, 메인 스레드 블로킹, `--version` 무시)은 전부 **수동 확인으로만
잡혔고 자동 검증이 전무했다.** 이번 릴리스는 기능 추가가 아니라 재발 방지용
테스트 인프라.

### Added
- **`scripts/smoke-test.sh`** — 실제 바이너리를 빌드해 프로세스 생명주기를
  검증하는 E2E 스모크 테스트. 유닛 테스트로는 못 잡는 버그 클래스를 정조준:
  - `--version`/`--help`가 실제로 출력하고 **바로 종료**하는지 (타임아웃 시
    실패 — GUI로 떠서 행업되는 걸 5초 안에 잡음. `peterfan-menubar`의
    과거 버그가 정확히 이 패턴)
  - `--mock` 모드에서 15개 읽기 전용 커맨드가 크래시 없이 도는지
  - `--json` 출력이 실제로 유효한 JSON인지 (`python3 -m json` 파싱)
  - `peterfand --mock --once`가 커브를 적용하고 정상 종료하는지
  - `peterfan-menubar --mock`이 시작 후 크래시 없이 살아있다가 SIGTERM에
    깔끔히 종료하는지
  - 총 31개 체크, CI(`.github/workflows/ci.yml`)에 연결해 매 커밋마다 실행.
  - 이 하네스를 만들자마자 **`peterfan-tui`에 `--version`/`--help` 처리가
    아예 없던 버그**를 즉시 발견 — menubar와 동일한 버그 클래스가 TUI에도
    있었음. 같이 수정.
- **`tray_attributes_route_clicks_correctly`** 유닛 테스트 — v1.9.3에서 고친
  "좌클릭이 팝오버 대신 항상 메뉴를 띄우던" 버그의 회귀 테스트. `muda::Menu`가
  메인 스레드에서만 생성 가능해 실제 OS 객체 없이도 테스트 가능하도록
  `menu_on_left_click`/`menu_on_right_click` 판단 로직을 `click_routing()`
  순수 함수로 분리.

### Fixed
- `cargo fmt --all -- --check`와 `cargo clippy -- -D warnings`(CI가 실제로
  거는 엄격 모드)가 이전부터 조용히 실패하고 있었음 — 워크스페이스 전체
  포맷팅 정리, `cmd_alert`의 `too_many_arguments`와 불필요한 `return` 정리.

## [1.9.5] — 팝오버 오픈 애니메이션 정리 + Fan Control 칩 레이아웃 수정

v1.9.3 좌클릭 수정 이후 실제 팝오버가 처음으로 사용자에게 보였고, 그 결과
드러난 폴리싱 이슈 두 가지.

### Fixed
- **"Fan Control" 칩 7개(Auto/Rules/Silent/Balanced/Gaming/Performance/Maximum)가
  3+3+1로 줄바꿈되면서 마지막 "Maximum" 칩만 전체 폭으로 어색하게 늘어나던
  버그** — flex-wrap의 stretch 동작 때문. `display:grid;grid-template-columns:
  repeat(3,1fr)`로 교체해 마지막 줄이 남는 칸을 채우지 않고 자기 폭만 차지하도록 수정.
- **팝오버가 열릴 때 애니메이션과 창 크기 재조정이 겹쳐서 "두둥"거리며 튀는
  버그** — 콘텐츠 실제 높이를 측정해 창 크기를 맞추는 로직이 페이드인
  애니메이션 도중에 실행되면서 창이 애니메이션 중간에 갑자기 커지는 것처럼
  보였음. 두 번째 이후 열 때는 마지막으로 측정된 높이로 **보이기 전에 미리
  맞춰서** 열도록 수정(최초 1회만 남아있는 리사이즈 발생). 애니메이션 자체도
  scale/translate 오버슈트를 제거하고 더 짧고 차분하게 조정.

## [1.9.4] — 팬 제어 명령이 메인 스레드를 막던 버그 수정 (버벅임)

### Fixed
- **모든 팬 제어 명령이 UI 이벤트 루프에서 동기적으로 실행되던 문제** —
  우클릭 메뉴(Auto/Rules/프로필/Fan Speed)나 팝오버 칩을 누르면 SMC 호출이
  끝날 때까지 메뉴바 전체가 멈췄다. 데몬 미설치 + 비루트 상태에서는 SMC 쓰기
  시도가 실패하는 과정 자체가 수백 ms 걸릴 수 있어 "Maximum 눌러도 반응 없고
  버벅인다"는 증상으로 나타남.
  `App.provider`를 `Box`에서 `Arc<dyn HardwareProvider>`로 바꿔 백그라운드
  스레드에서 안전하게 공유하도록 하고, 실제 하드웨어 I/O(`execute_control`)를
  전부 `std::thread::spawn`으로 이동. 메뉴 클릭은 이제 즉시 반환되며, 결과는
  완료되는 대로 알림(우클릭 메뉴) 또는 팝오버 상태줄(다음 1초 틱)에 반영된다.

## [1.9.3] — 팝오버가 한 번도 열릴 수 없었던 치명적 버그 수정

### Fixed
- **좌클릭이 항상 우클릭 메뉴를 띄우던 버그** — `tray-icon` 크레이트는
  `.with_menu()`로 메뉴를 붙이면 **좌클릭에서도 기본적으로 그 메뉴를
  띄운다**(`with_menu_on_left_click` 기본값 `true`). PeterFan은 이 옵션을
  끈 적이 없어서, 좌클릭 시 우리 자체 `TrayIconEvent::Click` 핸들러(팝오버
  토글)가 실행될 기회조차 없었다. 결과적으로 **v1.7~v1.9에서 만든 팝오버
  대시보드(히스토리 그래프, 라이선스 상태, 라이트/다크 모드, 오픈 애니메이션
  전부)가 실제 사용자에게 한 번도 도달한 적이 없었다** — 항상 우클릭
  메뉴만 보였던 것.
  `.with_menu_on_left_click(false)`를 명시적으로 설정해 좌클릭은 팝오버,
  우클릭은 네이티브 메뉴로 정확히 분리.
- 실제 macOS 화면을 스크린샷으로 캡처해 메뉴바 아이콘 존재 여부와 클릭
  동작을 직접 검증하는 과정에서 발견 — 이전까지는 프로세스 생존 여부만
  확인했을 뿐 시각적으로 검증한 적이 없었다.

## [1.9.2] — 팬 속도 설정이 실제로 작동하지 않던 버그 수정

### Fixed
- **`apply_local()`이 `"hold:<pct>"` 명령을 전혀 인식하지 못하던 버그** —
  데몬(`peterfand`)이 설치되어 있지 않을 때 Fan Speed 프리셋(25/50/75/100%)이
  직접 SMC 제어로 폴백하는데, 이 폴백 경로가 "hold:" 명령 자체를 몰라서 항상
  "unknown command"로 조용히 실패했다. `auto`/`profile:`과 동일한 패턴으로
  `hold:<pct>` 처리를 추가. 유닛 테스트로 회귀 방지.
- **우클릭 메뉴 명령이 실패해도 아무 피드백이 없던 버그** — Auto/Rules/프로필/
  Fan Speed를 팝오버 없이 우클릭 메뉴에서 실행하면 결과가 팝오버 내부의
  상태 텍스트에만 기록되어, 팝오버를 열어보지 않는 한 성공/실패 여부를 전혀
  알 수 없었다. 이제 macOS 데스크탑 알림으로 즉시 결과 표시.

### 근본 원인 메모
사용자 환경에서 실제로 안 되던 이유는 두 가지가 겹쳐 있었다: (1) `peterfand`
데몬이 설치되지 않아 메뉴바 앱이 SMC에 직접 쓸 권한이 없었고, (2) 위 두
버그로 인해 그 실패가 완전히 침묵 처리됐다. `peterfan doctor`가 "daemon
reachable: ✗ / running as root: ✗"를 정확히 보고하고 있었음 —
`peterfan install-daemon` 실행이 근본 해결책.

## [1.9.1] — 우클릭 메뉴에서 팬 속도 직접 설정

### Added
- **"Fan Speed" 서브메뉴** — 우클릭 메뉴에 Auto + 25% / 50% / 75% / 100%
  프리셋 추가. 프로필(Silent/Balanced/...)을 거치지 않고 팝오버를 열지 않아도
  바로 특정 % 로 고정 가능. 기존 `execute_control`의 "hold:<pct>" / "auto"
  커맨드 경로를 그대로 재사용(데몬 우선, 없으면 직접 SMC 제어로 폴백).

## [1.9.0] — 메뉴바 설정(지표/표시 방식) + --version 버그 수정

### Fixed
- **`peterfan-menubar`가 `--version`/`--help`를 무시하고 그냥 실행되던 버그** —
  버전 확인용 호출이 그대로 두 번째 메뉴바 아이콘을 띄우는 사고로 이어졌음.
  이제 다른 바이너리와 동일하게 즉시 출력 후 종료.

### Added — 메뉴바 커스터마이징 (iStat 스타일 3단계)
- **`peterfan_core::config::{MenubarMetric, MenubarDisplay, MenubarConfig}`** —
  메뉴바가 무엇을(CPU/메모리/온도/팬/네트워크) 어떻게(숫자만/그래프만/숫자+그래프)
  보여줄지 config TOML에 영속화. 기본값(CPU + 숫자와그래프)일 때는 TOML에
  섹션 자체가 안 써짐.
- **우클릭 메뉴에 "Menu Bar Shows" / "Menu Bar Style" 서브메뉴** 추가 —
  네이티브 체크메뉴 아이템으로 지표·표시방식을 즉시 전환, 선택 즉시 config에
  저장되어 재실행 시에도 유지됨. (팝오버 웹뷰 대신 네이티브 메뉴로 구현 —
  더 안정적이고 macOS 룩앤필에 자연스러움.)
- **팬 히스토리 버퍼(`fan_hist`) 신설** — 팬을 메뉴바 지표로 선택하면 최근
  2분간 RPM 추이(정격 대비 %)를 스파크라인으로 표시.
- **네트워크 메뉴바 그래프** — rx+tx 합산, 고정 상한 없이 최근 구간 자체
  최대치 기준으로 자동 스케일링.
- `peterfan login-item install --metric` 이 `memory`/`network` 값도 인식.

### Changed
- 메뉴바 그래프 아이콘 생성 함수가 지표에 무관하게 재사용되도록 일반화
  (`&[f32]` + `Option<max>` 시그니처로 CPU/메모리/온도/팬/네트워크 공용).
- 팬 데이터가 이제 매 틱 무조건 수집됨(기존엔 팝오버가 열려 있을 때만) —
  팬을 메뉴바 지표로 선택했을 때 그래프가 팝오버 닫힘 상태에서도 계속 갱신됨.

### Note
- 팬 조절(Auto/Rules/프로필/Hold) 기능 자체는 이전 버전과 동일하게 작동함
  (우클릭 메뉴의 Auto/Rules/프로필 항목 + 팝오버의 Hold 슬라이더). 이번
  릴리스는 메뉴바 "표시" 설정을 추가한 것이며 팬 제어 로직은 변경 없음.

## [1.8.0] — 라이선싱 인프라 + 센서 심층화 + UI 폴리싱 (iStat 스타일 2/3단계)

"돈벌거야" 방향 전환의 2·3단계: 오프라인 검증 가능한 라이선스 키 체계와
14일 무료 체험판을 도입하고, 실제 M3 Max에서 검증한 배터리 온도 센서를 추가했다.

### Added — 라이선싱
- **`peterfan_core::license`** — Ed25519 서명 기반 오프라인 라이선스 키.
  키 형식: `PFAN1-<base64url(JSON payload)>.<base64url(signature)>`.
  검증은 바이너리에 내장된 공개키만으로 완결되며 서버가 필요 없다.
  개인키는 저장소에 없음 — `tools/license-keygen`(워크스페이스 제외, 미배포)이
  발급 전용 도구이며, 실제 키페어를 생성해 공개키만 코드에 내장했다.
- **14일 무료 체험판** — `Config.license.first_run_unix`에 최초 실행 시각을
  기록(메뉴바 앱 또는 데몬 중 먼저 실행되는 쪽이 기록, 공유 시계).
  CLI의 읽기 전용 커맨드(`status`, `temps`, `fan set` 등)는 절대 게이팅되지
  않음 — 유료 대상은 **메뉴바 앱 상시 실행 + 데몬 영구 팬 제어**뿐.
- **`peterfan license status|activate <key>|deactivate`** — 체험판 잔여일 또는
  라이선스 이메일/만료일 확인, 키 등록/해제.
- **메뉴바 팝오버 라이선스 UI** — 체험판 잔여일 표시 + "Activate" 토글 폼.
  체험판 만료 시 배너가 강조되고 폼이 자동으로 펼쳐지며 "Buy License →" 링크
  노출(현재 플레이스홀더 URL — 실제 결제 페이지 연결은 별도 작업 필요).
- **데몬 만료 게이트** — 체험판 만료 + 라이선스 없음 상태에서는 사용자가
  어떤 모드를 선택했든 매 틱 자동(OS 관리) 제어로 폴백. 읽기 전용 IPC(`temps`,
  `status`)는 계속 응답 — 온도/팬 캐시를 이제 auto/비auto 무관하게 매 틱 갱신.

### Added — 센서 심층화
- **배터리 온도 센서** — macOS 백엔드가 IOHID의 "gas gauge battery" 판독값을
  평균 내어 `SensorKind::Battery`로 노출. 기존에는 완전히 버려지던 데이터.
  M3 Max 실기에서 검증(29~31°C, 유휴 상태에 그럴듯한 값).
  Mock 백엔드에도 대응 센서 추가.
- IOHID 원시 센서 이름을 실기에서 덤프해 확인한 결과, Apple Silicon은
  P-core/E-core/GPU를 구분 가능한 이름으로 노출하지 않음(익명 "PMU tdieN"
  인덱스뿐) — 클러스터별 표시는 신뢰할 수 없는 추측이 되므로 보류하고,
  검증 가능한 배터리 센서만 추가했다(정확성 우선).

### Fixed — UI 폴리싱
- **팝오버 라이트/다크 모드 자동 대응** — `prefers-color-scheme` 기반으로
  패널/텍스트/트랙 색상 전환. 기존엔 다크 고정.
- **팝오버 오픈 애니메이션** — 스케일+페이드인 트랜지션 추가, 드롭섀도 강화.

### Changed
- `SensorKind`에 `Battery` variant 추가(기존 `Other`에서 분리).


"돈벌거야" 방향 전환의 1단계: iStat Menus 급 상용 품질을 향한 첫 걸음으로,
메뉴바의 "한눈에 보이는" 시그니처 기능인 실시간 그래프를 도입했다.

### Fixed
- **메뉴바 팝오버 JS 치명적 버그** — 팬 컨트롤 칩 활성화 표시, hold 슬라이더
  동기화, 데몬 상태 텍스트 로직 전체가 스마트/curly 따옴표(‘’)로 작성되어 있어
  JS 파싱 자체가 깨져 있었다(아마 에디터 자동교정으로 유입). 전부 표준 따옴표로
  수정 — 이 기능들은 이번 릴리스 전까지 실질적으로 동작하지 않고 있었다.

### Added
- **메뉴바 아이콘 스파크라인** — CPU 사용률 추이를 막대그래프로 렌더링해
  메뉴바 아이콘 자체에 표시. 현재 부하 구간(초록/노랑/빨강)에 따라 색이 바뀜.
  기존 고정 링 아이콘을 대체 — iStat의 "그래프가 곧 아이콘" 스타일.
- **팝오버 히스토리 차트** — CPU / 메모리 / 온도 / 네트워크 처리량에 최근
  2분간(120틱 @ 1Hz) 추이를 보여주는 인라인 canvas 영역 그래프 4개 추가.
  외부 차트 라이브러리 없이 순수 canvas 2D API로 구현(자체 포함 유지).
- **롤링 히스토리 버퍼** — `App`에 `cpu_hist`/`mem_hist`/`temp_hist`/
  `net_rx_hist`/`net_tx_hist` (VecDeque, 용량 120) 추가. 팝오버가 닫혀 있어도
  매 틱 갱신되어 다시 열었을 때 즉시 히스토리가 채워진 그래프를 볼 수 있음.

### Changed
- `update()`가 메모리/온도/네트워크를 팝오버 가시성과 무관하게 매 틱 수집하도록
  재구성(히스토리 버퍼 유지 목적). 언더레잉 sysinfo refresh는 이미 매 틱
  발생하고 있었으므로 CPU/메모리/디스크/네트워크 수집 자체는 추가 비용 없음;
  온도 센서 읽기만 상시화됨(공급자가 이미 초기화되어 있어 저비용).

## [1.6.0] — 경량 모니터 + 데몬 통합 최적화

### Performance
- **`quick_monitor()` — 경량 sysinfo 백엔드** 도입.
  `memory`, `battery`, `system`, `doctor` 커맨드 전용으로, 프로세스 열거 및
  디스크/네트워크 초기화를 완전 생략. **실제 효과: 177ms → 5ms (35배 빠름)**.
- **`peterfan temps` / `peterfan fans` 데몬 패스스루** — 데몬이 실행 중이면
  SMC 초기화 없이 캐시 데이터를 IPC로 즉시 반환.
  **실제 효과: ~60ms → ~1ms (60배 빠름)** (데몬 실행 중)
- **`peterfan status` 데몬 실행 중 프로바이더 초기화 제거** — 온도/팬/전력/백엔드
  정보를 모두 `temps` IPC에서 가져와 하드웨어 프로바이더(SMC) 초기화 자체를 건너뜀.
  기존: `sampled_monitor(150ms) + provider_init(~60ms)` 직렬 → 현재: 150ms만.
- **IPC 왕복 1회 절감** — 데몬 `temps` 응답에 `mode`, `backend`, `power_w` 포함.
  `status`, `status --compact`, `fans` 커맨드가 별도 `status` IPC 호출 없이
  팬 제어 모드를 표시.

### 커맨드별 체감 레이턴시 (release 빌드, 터미널 기준)

| 커맨드 | v1.5.0 | v1.6.0 | 개선 |
|---|---|---|---|
| memory / battery / system / doctor | 177ms | **5ms** | **35x** |
| temps / fans (데몬 없음) | 62ms | 62ms | — |
| temps / fans (데몬 실행 중) | 62ms | **~1ms** | **60x** |
| status (데몬 없음) | 170ms | 170ms | — |
| status (데몬 실행 중) | 170ms | **~150ms** | 1.1x |
| cpu / disk / network / top | 165ms | 165ms | — |

## [1.5.0] — CLI 성능 전면 최적화

### Performance
- **`instant_monitor()`** 도입 — delta 샘플링이 불필요한 커맨드(`memory`, `battery`,
  `system`, `doctor`)는 300ms sleep 없이 즉시 응답.
  실제 효과: **300ms → 6ms** (50배 빠름)
- **`SAMPLE_MS` 300 → 150ms** — CPU%, 디스크/네트워크 속도 등 delta가 필요한
  커맨드도 2배 빠르게. CPU 사용률 정밀도는 150ms에서도 충분(≥1% 단위).
- **`sampled_monitor_and_provider()` 병렬 초기화** — `status` 커맨드 실행 시
  하드웨어 프로바이더(SMC) 초기화와 모니터 샘플 sleep이 동시에 진행.
  `HardwareProvider: Send + Sync` 보장으로 안전하게 구현.
  실제 효과: **350ms → 150ms** (2.3배 빠름)
- **데몬 센서 캐시 IPC** — 데몬이 매 틱 읽은 온도/팬 데이터를 `State`에 캐시.
  새 `temps` IPC 커맨드로 CLI에 즉시 전달. 데몬이 실행 중일 때 `status` 커맨드가
  SMC 초기화를 완전히 건너뜀. 실제 효과: **데몬 실행 중 status ≈ 10ms**

### 커맨드별 체감 레이턴시 (release 빌드, /dev/null 기준)

| 커맨드 | v1.4.0 | v1.5.0 | 개선 |
|---|---|---|---|
| memory / battery / system / doctor | 300ms | 6ms | **50x** |
| cpu / disk / network / top | 300ms | 150ms | **2x** |
| status (데몬 없음) | 350ms | 170ms | **2x** |
| status (데몬 실행 중) | 350ms | ~10ms | **35x** |

## [1.4.0] — Alert config + LaunchAgent + config set/get 확장

### Added
- **`[alert]` config 섹션** — 알림 임계값을 TOML에 영구 저장.
  `peterfan alert --cpu 85 --temp 90 --save` 또는
  `peterfan config --set alert.cpu 85` 로 설정.
- **`peterfan alert --save`** — CLI 플래그를 config에 저장하고 종료.
  이후 `peterfan alert` 는 플래그 없이도 저장된 임계값으로 실행.
- **`peterfan alert install/status/remove`** — 사용자 LaunchAgent 관리.
  로그인 시 `peterfan alert` 를 자동 실행해 백그라운드에서 상시 모니터링.
- **`peterfan config --set/--get alert.*`** — alert 서브키 직접 편집.
  지원 키: `alert.cpu`, `alert.memory`, `alert.temp`, `alert.cooldown`, `alert.interval`
- **`peterfan config`** 출력에 Alert 섹션 추가 — 현재 임계값 요약 표시.

## [1.3.0] — 배포 인프라 완성 + alert 명령어

### Added
- **`peterfan alert`** — CPU / 메모리 / 온도 임계값 초과 시 데스크탑 알림.
  - `--cpu <pct>`, `--memory <pct>`, `--temp <°C>` 로 임계값 설정
  - 알림은 macOS `osascript`(Funk 사운드), Linux `notify-send` 사용
  - `--cooldown <secs>` (기본 300s): 동일 지표의 반복 알림 억제
  - `--once`: 한 번만 체크하고 종료 — 임계값 초과 시 exit code 1 (cron/스크립트 연동)
  - 기본 모드: 인터벌마다 상태 표시, 초과 시 즉시 알림 발송

### Changed
- **Release workflow** 전면 개선:
  - Universal macOS 바이너리(`lipo` arm64 + x86_64)로 단일 아카이브 제공
  - SHA256 체크섬 파일(`checksums.txt`) 릴리즈에 자동 첨부
  - CHANGELOG.md에서 해당 버전 섹션 자동 추출 → GitHub Release 노트로 사용
  - `workflow_dispatch` 입력으로 수동 태그 릴리즈 지원

### Added (packaging)
- **Homebrew formula** — `packaging/homebrew/peterfan.rb`: `brew install` 지원.
  릴리즈 후 SHA256 갱신만 하면 바로 배포 가능.

## [1.2.1] — profile/curve 커맨드 개선

### Changed
- **`peterfan profile`** — 목록에 정의된 custom + named curves도 함께 표시.
  커스텀 곡선은 청록색으로 구분.
- **`peterfan curve <name>`** — 커스텀 곡선 이름으로 곡선 시각화 지원.
  예: `peterfan curve custom`, `peterfan curve work`

## [1.2.0] — 사용자 정의 팬 곡선 (Custom Curve)

### Added
- **`peterfan profile create <name> --points "30:20,60:50,80:90,90:100"`**
  — 온도:duty 쌍으로 커스텀 팬 곡선을 config 파일에 저장. 이름이 `custom`이면
  `profile = "custom"` 기본 슬롯에, 다른 이름이면 named curve로 저장.
- **`peterfan profile delete <name>`** — 커스텀 곡선 삭제.
- **`peterfan profile list`** — 정의된 커스텀 곡선 목록 출력.
- **Config `[custom_curve]` 섹션** — TOML에서 직접 정의 가능:
  ```toml
  [custom_curve]
  points = [[30, 20], [60, 50], [80, 90], [90, 100]]
  ```
- **`[named_curves.<name>]` 섹션** — rules에서 이름으로 참조 가능한 추가 곡선.
- **데몬 `Profile::Custom` 실제 적용** — `config.curve_for()` 사용으로
  custom 프로파일 선택 시 사용자 정의 곡선이 실제로 적용됨.

## [1.1.0] — 메뉴바 우클릭 네이티브 컨텍스트 메뉴

### Added
- **메뉴바 우클릭 네이티브 메뉴** — 팝오버를 열지 않고도 바로 팬 모드 전환.
  메뉴 구성: Auto · Rules · — · Silent / Balanced / Gaming / Performance / Maximum · — · Quit
- **좌클릭/우클릭 구분** — 좌클릭은 기존처럼 팝오버 토글, 우클릭은 네이티브 메뉴 표시.
  선택 즉시 IPC로 데몬에 명령 전송 후 팝오버 상태도 갱신.

## [1.0.0] — 정식 릴리즈

### 주요 변경 (0.30.0 → 1.0.0)

#### Added
- **`peterfan doctor` 전면 강화** — LaunchDaemon 로딩 상태(`launchctl list`),
  config 파일 유효성 검사(잘못된 규칙 조건 경고), config 요약 표시, 버전 번호 추가.
- **`peterfan update --check`** → `peterfan update`로 단순화. GitHub 최신
  릴리즈와 현재 버전 비교; 업데이트 시 `cargo install peterfan` 안내.
- **CLI 레퍼런스 문서 전면 갱신** (`docs/CLI.md`) — `watch`, `update`, `rule`,
  `daemon`, `config --set/--get`, `benchmark --profile` 모두 추가.

### 전체 기능 요약 (v1.0.0)

| 기능 | 커맨드 |
|---|---|
| 시스템 모니터링 | `status`, `cpu`, `memory`, `disk`, `network`, `top`, `battery`, `system` |
| 열 측정 | `temps`, `fans`, `hardware` |
| 팬 제어 | `fan set/auto/status`, `profile`, `curve` |
| 실시간 모니터링 | `watch`, `tui` (별도 바이너리) |
| 자동화 | `rule add/remove/clear`, `config --set` |
| 데몬 관리 | `daemon status/reload/stop/log`, `install-daemon` |
| 메뉴바 | `login-item install/remove`, `--metric cpu/temp/fan` |
| 진단 | `doctor`, `update` |
| 개발자 도구 | `log`, `benchmark`, `serve`, `completions` |

## [0.30.0] — watch + update + 데몬 reload 버그 수정

### Added
- **`peterfan watch`** — CPU%, MEM%, 온도, RPM, 전력, 데몬 모드를 한 줄에
  색상으로 표시하며 주기적으로 갱신. Ctrl-C로 종료. `-i N`으로 갱신 주기 설정.
- **`peterfan update`** — GitHub 최신 릴리즈와 현재 버전 비교. 업데이트가
  있으면 `cargo install peterfan` 명령을 안내.

### Fixed
- **데몬 `reload` 후 `interval`/`critical` 미반영 버그 수정** — 기존에는
  `peterfand --interval 5`로 시작한 값이 `reload` 후에도 그대로 유지됐으나,
  이제 매 틱마다 `state.config`에서 값을 읽어 `reload` 즉시 반영됨.

## [0.29.6] — Menubar popover: Rules + Hold slider + active-mode highlight

### Added
- **Popover Rules button** — switches daemon to rules mode from the menu-bar UI.
- **Hold slider** — drag the 0–100% range slider and click "Set" to send
  `hold <n>` to the daemon. While the slider is not focused the position
  auto-syncs to the daemon's current hold %, so re-opening the popover always
  shows the live value.
- **Active-mode highlighting** — the button matching the current daemon mode
  (auto, rules, a profile, or hold) is highlighted in blue on every tick so
  the popover always shows at-a-glance what mode is active.

## [0.29.5] — TUI hold-% input + rules key

### Added
- **TUI `h` key** — enters an inline hold-input prompt in the footer bar. Type a
  duty % (0–100), press Enter to send `hold <n>` to the daemon, or Esc to cancel.
  The prompt renders with a highlighted yellow bar and a blinking cursor.
- **TUI `r` key** — switches the daemon to rules mode directly from the TUI.
- Footer now lists all fan-control keys: `1-5 · a · r · h`.

## [0.29.4] — `benchmark --profile` with daemon restore

### Added
- **`peterfan benchmark --profile <name>`** — applies a named fan profile before
  the stress run and automatically restores the previous daemon mode (hold, auto,
  rules, or manual profile) when the benchmark finishes.
- JSON output now includes `"profile"` key (applied profile or null) alongside
  existing fields.
- Text output shows the active profile and prints a restore confirmation line.

## [0.29.3] — Log rotation + doctor Setup section

### Added
- **`install-daemon` now writes `/etc/newsyslog.d/peterfand.conf`** — macOS log
  rotation for `/var/log/peterfand.log` (≥1 MB → rotate, keep 5 bzip2
  archives) and `/var/log/peterfand.err` (≥512 KB, keep 3). `uninstall-daemon`
  removes it.
- **`peterfan doctor` Setup section** (macOS) — now checks:
  - Whether the menubar login item is installed (and suggests the install command)
  - Whether the daemon state file exists and shows the saved mode
  - Log file presence, size, and whether log rotation is configured

## [0.29.2] — `peterfan status --compact` + TUI fan duty% + log-on-change

### Added
- **`peterfan status --compact`** — one-line summary for shell prompts and
  status bars: `CPU 23% | MEM 41% | 47°C | 2100 RPM | hold:80%`.
- **TUI fan panel** now shows duty % (yellow) alongside RPM when available.

### Changed
- `peterfand` only logs when fan duty or control mode changes (see v0.29.1).

## [0.29.1] — Daemon log-on-change: only writes when duty or mode changes

### Changed
- `peterfand` now only logs when the fan duty or control mode actually changes.
  Previously it logged every 2 s tick (43k lines/day); now a steady state at a
  fixed duty produces zero log growth. Changes, critical overrides, and IPC
  commands are still logged.

## [0.29.0] — `peterfan daemon log` — tail the fan-control daemon log

### Added
- **`peterfan daemon log`** — print the last 40 lines of `/var/log/peterfand.log`
  (the LaunchDaemon's stdout). `-n N` to change line count; `-f`/`--follow` for
  continuous tailing (Ctrl-C to stop). Ideal for diagnosing fan-curve decisions
  and IPC commands.

## [0.28.9] — `peterfan config --get` for reading single config values

### Added
- **`peterfan config --get <key>`** — print a single config value as a plain
  string (or JSON with `--json`). Useful in scripts:
  ```bash
  PROFILE=$(peterfan config --get profile)
  ```

## [0.28.8] — UX polish: `peterfan rule` lists by default + fans shows daemon mode

### Changed
- **`peterfan rule`** (no subcommand) now lists rules instead of showing help.
- **`peterfan fans`** now shows the daemon's current control mode (`hold:80%`,
  `rules:balanced`, `auto`, …) as a bullet above the fan table when a daemon
  is running.

## [0.28.7] — Configurable menu-bar metric (CPU / temp / fan RPM)

### Added
- **`peterfan-menubar --metric <cpu|temp|fan>`** — choose what to show in
  the macOS menu bar:
  - `cpu` (default) — CPU usage % as before
  - `temp` — hottest temperature sensor in °C  
  - `fan` — fastest fan speed in RPM
- **`peterfan login-item install --metric <cpu|temp|fan>`** — embeds the
  `--metric` flag into the LaunchAgent plist so the choice persists across
  reboots.

## [0.28.6] — `peterfan login-item` — menubar auto-start at login

### Added
- **`peterfan login-item install`** — writes a LaunchAgent plist to
  `~/Library/LaunchAgents/dev.peterfan.menubar.plist` and loads it
  immediately so `peterfan-menubar` starts at next login (and right now).
  Auto-discovers the sibling binary; `--binary <path>` overrides it.
- **`peterfan login-item remove`** — unloads and removes the plist.
- **`peterfan login-item status`** — shows whether the item is installed
  and the binary it points to.

## [0.28.5] — `peterfan daemon` subcommand + live config reload

### Added
- **`peterfan daemon status`** — show the running daemon's fan-control mode.
- **`peterfan daemon reload`** — tell the daemon to re-read its config from
  disk immediately (new rules and profile default take effect within one tick).
- **`peterfan daemon stop`** — tell the daemon to shut down gracefully (fans
  restored to automatic before exit).
- **`peterfan config --set` and `peterfan rule add/remove/clear`** now
  automatically send `reload` to a running daemon, so config changes are live
  without restarting the daemon.
- Daemon `reload` and `stop` IPC commands added to `peterfand`.

## [0.28.4] — `peterfan rule` — automation rule management from the CLI

### Added
- **`peterfan rule list`** — list all rules with their index, condition, and profile.
- **`peterfan rule add <condition> <profile>`** — append a rule to the config.
  Validates the condition before writing. Example:
  ```
  peterfan rule add on_battery silent
  peterfan rule add "cpu_above:85" maximum
  peterfan rule add "time:22-7" silent
  ```
- **`peterfan rule remove <index>`** — remove a rule by its `list` index.
- **`peterfan rule clear`** — remove all rules.
  All write commands use `platform::config::save()` and print the file path.

## [0.28.3] — `peterfan config --set` for in-place config editing

### Added
- **`peterfan config --set <key> <value>`** — change a single config value
  without opening the TOML file. Supported keys: `profile`, `interval`,
  `critical`. Creates the file if missing. Examples:
  ```
  peterfan config --set profile gaming
  peterfan config --set interval 3
  peterfan config --set critical 95
  ```

## [0.28.2] — Daemon state persistence across reboots

### Added
- **`peterfand` saves its mode to disk** (`/Library/Application Support/peterfand/state.toml`
  on macOS, `/var/lib/peterfand/state.toml` on Linux) on every IPC state
  change (`hold`, `profile`, `auto`, `rules`). On next startup the last mode
  is restored — `hold:80%` survives a reboot without any extra `peterfan fan
  set` after boot. The startup log now includes `restored=<mode>`.

## [0.28.1] — `peterfan fan status` subcommand

### Added
- **`peterfan fan status`** — shows the current fan-control mode (daemon:
  `hold:N%` / `rules:…` / `auto` / `manual:profile`, or the local provider
  fallback) plus live RPM for every fan. Useful for scripting and quick checks
  without needing the full `peterfan status` output.

## [0.28.0] — Fan control without sudo + TUI keyboard fan control

### Added
- **`peterfan fan set N` no longer needs `sudo`** when `peterfand` is running:
  the command routes through the daemon IPC (`hold N%`) so the setting persists
  across reboots and the daemon re-asserts it every tick. Falls back to a direct
  SMC write (needs `sudo`) when no daemon is running.
- **`peterfan fan auto`** similarly routes through the daemon when available.
- **Daemon `hold <percent>` IPC command** — holds fans at a fixed duty until
  `auto`, `rules`, or `profile` clears it. `status` now reports `hold:N%`.
- **TUI fan control keyboard shortcuts** (when daemon is running or process has
  root): `1` silent · `2` balanced · `3` gaming · `4` performance · `5` maximum
  · `a` auto. Current daemon mode shown in the Thermals block title.
- **Menu-bar popover** shows the daemon's current mode (`rules:balanced`,
  `hold:80%`, `auto`) in real-time; shows an install-daemon tip when no daemon
  is present.
- **`peterfan status`** shows daemon mode below the Fans section.
- **HTTP API** (`peterfan serve`) fan and profile endpoints route through the
  daemon IPC when available.

### Changed
- `platform/ipc`: added shared `send_command()` helper used by CLI, TUI, and
  menu-bar — removes three copies of the same IPC logic.

## [0.27.1] — Fan-control sequence matched to a proven implementation

### Changed
- Byte-for-byte aligned the Apple Silicon unlock with the known-working
  reference (agoodkind/macos-smc-fan): after `Ftst = 1` we now wait ~0.5 s for
  the thermal servo to settle, then poll the mode key for up to ~10 s (was 4 s)
  until manual mode holds. Target RPM stays a native-endian `flt` (`F0Tg`); mode
  key casing (`F0Md`/`F0md`) auto-detected.
- The slow unlock+poll runs **at most once per connection**, so the daemon never
  burns ~10 s every tick on firmware that ignores manual control.
- `peterfan fan set N` prints an "Applying…" line so the multi-second unlock
  isn't mistaken for a hang.

Confirmed against this M3 Max via `doctor`: `F0Md` + `Ftst` keys are present, so
the sequence is applicable; physical confirmation needs one root run of
`peterfan fan set N` (it verifies by reading RPM back).

## [0.27.0] — One-prompt fan-control setup (like Macs Fan Control)

### Added
- **`peterfan install-daemon` / `uninstall-daemon`** — install the root
  fan-control helper with a single macOS password dialog (`osascript … with
  administrator privileges`), no Terminal `sudo`. After that the menu-bar buttons
  and `peterfan fan …` drive fans through the root daemon with no further
  prompts — the same model Macs Fan Control / TG Pro use. `--dry-run` prints the
  exact privileged script first.

### Why
Fan control fundamentally needs root; competitors just hide it behind a one-time
privileged helper. PeterFan already had the unprivileged-app + root-daemon
architecture — this makes installing that daemon a one-click, GUI-password step.

## [0.26.2] — `doctor` diagnoses fan-control readiness

### Added
- `peterfan doctor` now has a **Fan control readiness** section: running as root?
  `peterfand` reachable? and (macOS) a read-only SMC probe showing the fan mode
  key (`F0Md`/`F0md`), whether the `Ftst` unlock key and Intel `FS! ` key are
  present — plus a one-line verdict on how to actually drive the fans. Same data
  in `--json` under `fan_control`. Needs no root (reads key-info only).

## [0.26.1] — Apple Silicon fan control: the real unlock sequence

### Fixed
- Implemented the **`Ftst` unlock sequence** required to actually drive fans on
  Apple Silicon. A bare `F0Md = 1` is reverted by `thermalmonitord` after a few
  seconds; we now write `Ftst = 1`, poll `F0Md = 1` until it holds, set `F0Tg`
  (little-endian float), and clear `Ftst` on restore. Mode-key casing (`F0Md`
  vs M5's `F0md`) is auto-detected. (Based on community reverse engineering —
  see `docs/RESEARCH.md`.)

This is what was missing in 0.26.0, where control was un-gated but still used a
bare mode write that Apple Silicon firmware ignores. Verification (RPM
read-back) is unchanged, so `sudo peterfan fan set N` will report a real ✓/✗.

## [0.26.0] — Fan control: un-gated, root-aware, and *verified*

This release fixes the central problem: a fan controller that didn't control fans.

### Changed
- **Apple Silicon fan control is no longer disabled.** It was gated to Intel
  after early writes showed no effect — but those writes were never run as root
  (the SMC rejects non-root writes), and tools like Macs Fan Control/TG Pro do
  drive Apple Silicon fans. Control is now attempted wherever the SMC is present.
- **`peterfan fan set N` verifies the result.** It records fan RPM, writes, waits,
  then re-reads RPM and reports a real **✓ responded / ✗ no change** — instead of
  printing "ok" for a write the firmware may have ignored. The menu-bar buttons
  show daemon status the same way.
- **Clear root guidance.** Fan writes need root; `fan set` now says exactly that
  (`sudo peterfan fan set N`, or run the `peterfand` daemon) instead of a generic
  permission error.

### Note
Fan control requires **root**. Run `sudo peterfan fan set 80` (or install the
daemon) — the verification will tell you definitively whether your Mac honors
manual fan control.

## [0.25.2] — Menu-bar popover: no inner scroll, clearer fan-control state

### Fixed
- Popover no longer shows an inner scrollbar / "frame-in-a-frame" look: the
  window is sized to the exact content height (measured via `scrollHeight`
  after layout settles, reported only once real data has populated), and the
  body has `overflow:hidden`.

### Changed
- When fan control isn't available (Apple Silicon, where macOS governs the
  fans), the Fan-control section now explains *why* there are no speed buttons
  ("monitor-only" + a one-line note) instead of a terse footnote.

## [0.25.1] — Memory breakdown in `status`, docs polish

### Added
- `peterfan status` now shows the wired / active / compressed memory line
  (previously only in `peterfan memory`).

### Changed
- Docs: documented `benchmark`, `log`, and `completions` in `docs/CLI.md`;
  refreshed the README example output and feature matrix.
- GPU utilization investigated via IOReport and **deferred** rather than shipped
  inaccurate — see `docs/RESEARCH.md`. The plumbing lives behind the
  off-by-default `experimental-gpu` feature.

## [0.25.0] — Memory breakdown + CI

### Added
- **macOS memory breakdown** — wired / active / inactive / compressed bytes via
  the mach `host_statistics64(HOST_VM_INFO64)` call (the same source Activity
  Monitor uses). Shown in `peterfan memory` and exposed on the memory API.
  Cross-checked against `vm_stat`.
- **CI workflow** (`.github/workflows/ci.yml`) — `cargo fmt --check`, `clippy
  -D warnings`, and `cargo test` on every push / PR to `main`.

## [0.24.0] — Completions, logging, richer API

### Added
- **`peterfan completions <bash|zsh|fish|powershell>`** — shell completion
  scripts (clap_complete).
- **`peterfan log [--interval N] [--format csv|jsonl]`** — stream one metrics
  row per interval (time, cpu%, mem%, disk%, temp, fan rpm, power) for
  recording/piping (the spec's "Logs").
- HTTP API: **`GET /`** human-friendly index page and **`GET /api/v1/processes`**
  (top processes).

## [0.23.0] — Critical-temperature alerts

### Added
- The daemon now posts a **desktop notification** (macOS, via `osascript`) when
  the hottest temperature crosses the critical threshold — and another when it
  returns to normal (5°C hysteresis). Pairs with the existing force-to-100%
  safety override.

## [0.22.0] — Benchmark / stress mode

### Added
- **`peterfan benchmark [--secs N]`** — saturates every CPU core and samples
  CPU%, hottest temperature, fan RPM, and power once a second, then prints a
  summary (avg/peak CPU, peak temp, peak fan, peak power). `--json` too.
  Verified real: a short run drove CPU to 100%, power from ~24→35 W, and the
  fans up past 7000 RPM.

## [0.21.0] — TUI thermals panel

### Added
- The `peterfan-tui` dashboard now has a **Thermals** panel: temperature
  sensors (color-coded), fan RPMs, and total system power in the title. The TUI
  now reads the `HardwareProvider` alongside the `SystemMonitor`.

## [0.20.0] — Network IP & disk I/O

### Added
- **Per-interface local IP** and **per-disk read/write throughput** (bytes/s).
  `peterfan network` shows the IPv4 address; `peterfan disk` shows live `R …/s
  W …/s`. Both are in `--json`, `status`, and the HTTP API automatically.

## [0.19.0] — Automation rules

### Added
- **Automation rules** in the daemon: switch fan profile automatically by power
  source, temperature, or time of day. Configure in the TOML config:
  ```toml
  [[rules]]
  when = "on_battery"      # on_ac | on_battery | cpu_above:85 | time:22-7
  profile = "silent"
  ```
  Conditions are evaluated in order (first match wins); falls back to the base
  profile. The daemon reads power state and the local hour each tick.
- IPC gained `rules` (hand control back to automation) and `status` now reports
  the mode (`auto`/`manual`/`rules`). A manual `profile` over IPC overrides the
  rules until `rules`/`auto`. `peterfan config` lists the rules.

## [0.18.0] — Local HTTP API (`serve`)

### Added
- **`peterfan serve`** — a local JSON HTTP API (localhost) so other tools
  (Stream Deck, Raycast, Hammerspoon, Home Assistant, scripts) can read metrics
  and drive fan profiles:
  - `GET /api/v1/{status,system,cpu,memory,disks,network,battery,temps,fans,power}`
  - `POST /api/v1/profile` `{"name":"gaming"}` · `POST /api/v1/fan` `{"action":"auto"|"set","percent":N}`
  - CORS-enabled; single-threaded with ~1s background refresh. `--port` (default 9847).
  Verified end-to-end with curl (status keys, profile/fan POST, 404).

## [0.17.0] — Honest fan-control capability

### Changed
- **Fan control is now reported honestly per platform.** On **Intel** Macs the
  SMC fan-write path is offered (needs root/daemon). On **Apple Silicon** the
  fans are governed by the system — the same SMC writes are accepted but have no
  effect — so `control_fans` is now `false` there: `doctor` shows `✗ control
  fans`, `fan set` explains it's unavailable, and the popover hides the control
  buttons and notes "system-governed on Apple Silicon". Monitoring (CPU/die
  temps/fan RPM/power/…) is unaffected and fully real.

Background: across earlier versions the SMC write path was verified correct
(`F0Md`=ui8, `F0Tg`=flt; `FS! ` absent on Apple Silicon) and the connection is
held open, yet the physical fan does not respond on Apple Silicon. Rather than
ship a control that does nothing, PeterFan now says so.

## [0.16.0] — System power (watts)

### Added
- **Real system power draw (W)** on macOS via the SMC (`power_system_total`).
  `peterfan status` shows a **Power** line and the menu-bar popover appends it
  to the CPU line (e.g. `4.1 GHz   load …   24.3 W`). `HardwareProvider` gained
  `power_watts()` (None where unsupported).

## [0.15.0] — Hold the SMC connection (Apple Silicon fan control)

### Changed
- **Fan control now keeps the SMC write connection open** instead of opening
  and closing it per write. On Apple Silicon a forced fan reverts to automatic
  as soon as the SMC connection closes, so a one-shot `fan set` had no lasting
  effect; the **daemon holds the connection open** and re-asserts the target
  each tick, which is the correct way to hold a forced speed.

### Diagnostics / honesty
- Verified the write encoding is correct on this hardware (`F0Md` = ui8,
  `F0Tg` = `flt`; `FS! ` is absent on Apple Silicon, size 0). Writes succeed
  without error. Whether the fan physically responds depends on the machine —
  use `sudo peterfand --profile maximum` (continuous) and watch the RPM. A
  one-shot `peterfan fan set` won't hold on Apple Silicon because the process
  exits and the connection closes.

## [0.14.0] — Per-sensor & per-fan detail; sturdier fan control

### Added
- The popover now lists **every temperature sensor and every fan on its own
  line** (CPU / CPU-hottest / SSD / Airport / palm-rest …, and Fan 1 / Fan 2 …
  each with its own speed bar) instead of one truncated summary line — so
  machines with multiple CPU-die clusters or multiple fans show all of it.

### Changed
- Fan forcing now also flips the `FS! ` manual-mode bitmask (in addition to
  `Fn Md`), which some Macs require for `Fn Tg` to take effect. Best-effort:
  skipped where the key is absent. (Real-fan efficacy depends on the machine /
  SMC and needs a root daemon to exercise.)

## [0.13.2] — Daemon backend tag

### Changed
- The daemon now tags its IPC replies with its backend, e.g.
  `ok maximum (macos)` vs `ok maximum (mock)`. The popover's "Fan control"
  status shows it, so a **simulated (`mock`) daemon** can't be mistaken for one
  that actually drives the hardware — pressing a profile only moves real fans
  when a real (root) daemon is running.

## [0.13.1] — Popover control buttons always respond

### Fixed
- The popover control buttons did nothing (and gave no feedback) when no daemon
  was running. Now each button: (1) sends to the daemon if one is running and
  shows its reply, or (2) falls back to controlling fans directly via this
  process, or (3) shows a clear status (`start peterfand (needs root)`). A
  "Fan control" status line in the popover reflects the result of every click.

## [0.13.0] — Menu-bar ↔ daemon control (IPC)

### Added
- **Control buttons in the popover** — Auto / Silent / Balanced / Gaming /
  Performance / Max. They send a command to the running `peterfand` daemon over
  a Unix socket, so the menu-bar app (no privileges) can change the fan profile
  while the root daemon performs the SMC writes — **no per-action sudo**.
- **`peterfand` IPC server** (`platform::ipc`): line protocol `profile <name>` /
  `auto` / `ping` / `status` over `/var/run/peterfand.sock` (falls back to
  `/tmp`). The daemon switches profile / hands fans to the OS live; verified
  end-to-end. The socket is world-accessible (local-trust convenience).

## [0.12.0] — Watch mode & config file

### Added
- **`--watch [--interval N]`** — re-run any command on an interval, clearing
  the screen each time (a lightweight live monitor for `status`, `cpu`, `top`, …).
- **TOML config** at `~/.config/peterfan/config.toml` (platform config dir):
  `profile`, `interval_secs`, `critical_temp_c`. New `peterfan config [--init]`
  shows the path/values and writes a default file. The daemon and `--watch` now
  read their defaults from it (explicit flags still win).
- `Config` lives in `peterfan-core` (pure data + TOML); path/IO in
  `peterfan-platform::config`.

## [0.11.0] — Real CPU die temperature (Apple Silicon)

### Added
- **Real CPU/GPU die temperatures on Apple Silicon** via IOKit's IOHID
  temperature-sensor API (the SMC doesn't expose these). `peterfan temps` /
  `status` now show a real **CPU** temperature (average of the die sensors)
  plus **CPU hottest** and **SSD** (NAND), alongside the existing ambient SMC
  sensors. The menu-bar popover and the daemon's curve now key off the real CPU
  temperature.

### Notes
- Sensors are read by matching HID services on the Apple-vendor temperature
  usage page; the IOKit functions are private but exported by the framework.
  No root required.

## [0.10.0] — Fan-control daemon

### Added
- **`peterfand`** — a fan-control daemon that applies a profile's curve
  continuously (hottest temperature → curve → fan duty), with two safety
  behaviors:
  - **critical-temperature override** (`--critical`, default 90°C → 100% fans);
  - **restore-on-exit** — on `Ctrl-C`/`SIGTERM`/panic it returns the fans to
    automatic control, so it never leaves them forced.
  Flags: `--profile`, `--interval`, `--critical`, `--once`, `--mock`.
- **LaunchDaemon install** (`packaging/com.uulab.peterfan.daemon.plist` +
  `scripts/install-daemon-macos.sh`) so the daemon runs as root at boot — fan
  control then works without per-command `sudo`. (`peterfand` ships in macOS
  release archives.)

### Notes
- Running `peterfand` directly still needs root for SMC writes
  (`sudo peterfand`); the LaunchDaemon runs as root for you. `--mock` needs no
  privileges. Curve quality on Apple Silicon is limited until CPU/GPU die temps
  (IOHID) land — it currently keys off the hottest available sensor.

## [0.9.1] — Refined popover

### Changed
- Made the popover more compact and premium: tighter rows and padding, smaller
  uppercase section labels, lighter value weight with tabular-figure numerals,
  thinner bars, and subtler dividers.
- **The window now sizes itself to the content** — the WebView reports its real
  height and the window resizes to fit exactly (≈455px, down from 680), so
  there's no oversized panel or empty space.

## [0.9.0] — Fan control

### Added
- **Fan control on macOS** via SMC writes. New commands:
  - `peterfan fan set <pct> [--fan N]` — force fan(s) to a duty cycle.
  - `peterfan fan auto [--fan N]` — restore automatic (OS-managed) control.
  `peterfan profile <name>` now also applies on macOS.
- Implemented a minimal SMC write client (`smc_write`, IOKit) since `macsmc` is
  read-only. Duty % is mapped onto each fan's real `[min, max]` RPM range.

### Notes
- SMC writes are **privileged**: without root the kernel returns
  `kIOReturnNotPrivileged`, surfaced as a clear "re-run with `sudo`" error.
  Use `sudo peterfan fan set 60`.
- **Safety**: forced control persists until `fan auto` (or reboot) — the CLI
  warns about this on every `set`. Target RPM is clamped to the fan's rated
  range. A daemon with restore-on-exit / critical-temp ramp is future work.

## [0.8.1] — App icon

### Added
- A proper **app icon** for `PeterFan.app` — a white four-blade fan on a
  teal→sky→blue gradient squircle. Generated from `tools/icongen` (tiny-skia)
  into `assets/icon-1024.png`, turned into `assets/AppIcon.icns` by
  `scripts/make-icns.sh`, and bundled by `scripts/bundle-macos.sh`.

## [0.8.0] — Double-clickable .app + consistent precision

### Added
- **`PeterFan.app`** — a double-clickable macOS menu-bar agent bundle
  (`LSUIElement`, no Dock icon), assembled by `scripts/bundle-macos.sh` and
  attached to macOS releases. Drag to /Applications and open.

### Fixed
- The menu-bar CPU percentage and the popover's CPU value disagreed because
  they rounded to different precision (e.g. `43%` vs `42.8%`). Both now use one
  decimal, so they always match.

## [0.7.1] — Clean menu-bar title

### Fixed
- The menu-bar title showed a block-character CPU sparkline that smeared into a
  solid white bar at high load. Replaced it with a plain, always-readable CPU
  percentage (e.g. `42%`) next to the icon.

## [0.7.0] — Unified popover with temps & fans

### Changed
- **The popover is now the whole menu-bar UI** — both left- and right-click
  (two-finger) open it, so there's no more inconsistent native menu. Quit moved
  into the popover (a button, via WebView IPC).
- **Added Temperature and Fans sections** to the popover (real SMC data on
  macOS): hottest temperature with the rest in the sub-line, and per-fan RPM.
- Refined spacing, alignment, and typography (consistent padding, uppercase
  section labels, aligned values and bars).

## [0.6.0] — Real macOS temperatures & fans

### Added
- **Real temperature and fan readings on macOS via the SMC** (`macsmc`/IOKit),
  no privileges required. `peterfan temps` / `fans` / `status` now show genuine
  data instead of the simulated fallback. Fans report actual/min/max RPM.

### Notes
- Only sensors that return a plausible value are shown. On Apple Silicon the SMC
  doesn't expose CPU/GPU **die** temps (they read 0 and are filtered); sensors
  the chip does expose (airflow/airport, palm rest, memory) are reported.
  CPU/GPU die temps need the IOHID thermal API — a future milestone.
- Fan **control** (SMC writes) is not yet implemented; fans are read-only
  (`controllable: false`).

## [0.5.0] — Popover dashboard

### Added
- **Left-click the menu-bar icon for a clean popover dashboard** — a borderless
  WebView window (wry) rendering an HTML/CSS panel à la RunCat/Stats: CPU (with
  a live per-core bar chart), memory, storage, battery, and network, each with
  an icon, sub-stats, and a load-colored progress bar. It positions itself under
  the icon, refreshes once a second, and closes when it loses focus.
- Right-click still opens the native menu (same figures + Quit) as a fallback.

## [0.4.2] — Readable menu-bar rows

### Fixed
- Menu-bar dropdown rows were rendered dim/grey because every row was a
  *disabled* menu item (macOS dims disabled items). Data rows are now enabled
  so they render in full, readable color; the header stays a subtle title.

## [0.4.1] — Professional menu-bar UI

### Changed
- Polished the menu-bar dropdown to a proper mini-dashboard: each row now has a
  load-colored status dot, a `▕████░░░░░▏` block-bar gauge, and aligned figures
  — CPU (with a per-core sparkline row), memory, disk, network, and battery
  (battery row only shown when present). The header shows the CPU brand.
- The menu-bar title now shows a tiny CPU-usage sparkline next to the percentage.

## [0.4.0] — Menu-bar app

### Added
- **`peterfan-menubar`** — a macOS menu-bar app (à la Stats) that shows live
  CPU usage in the menu bar with a dropdown of CPU / memory / network detail and
  a Quit item, refreshing once a second from the shared `SystemMonitor`. Runs as
  an accessory app (no Dock icon) via `tray-icon` + `tao`. On Windows the same
  binary shows a system-tray icon with the metrics in its tooltip. `--mock`
  drives it from the simulated machine. Run with `cargo run -p peterfan-menubar`.

## [0.3.0] — System dashboard TUI

### Changed
- **`peterfan-tui` is now a full system dashboard.** It polls the
  `SystemMonitor` once a second and renders CPU (global gauge + per-core
  sparkline + frequency/load), memory, disk(s), aggregate network throughput,
  a live CPU-usage history sparkline, battery, and a top-process table. Quit
  with `q`/`Esc`/`Ctrl-C`; `--mock` drives it from the simulated machine.

## [0.2.0] — System metrics

### Added
- **Real, cross-platform system metrics** via the `sysinfo` crate (macOS,
  Windows, Linux): CPU usage (global + per-core), frequency, load average,
  memory & swap, mounted disks, network throughput, and top processes.
- **Battery** state via the `battery` crate: charge, state, cycle count, time
  remaining, vendor/model, energy rate. State-of-health is filtered when the
  underlying crate reports an implausible value (a known Apple Silicon quirk).
- New core seam: the `SystemMonitor` trait plus `metrics` types, alongside a
  real `SysinfoMonitor` and a simulated `MockMonitor`.
- New CLI commands: `cpu`, `memory` (`mem`), `disk` (`disks`), `network`
  (`net`), `top` (`proc`, `--mem`, `-n`), `battery`, `system`. `status` is now a
  full dashboard combining system metrics and thermals.
- Performance: the monitor keeps a single long-lived handle and refreshes only
  the metric families it exposes (not `refresh_all`), tracking the sample
  interval to convert byte deltas into per-second network rates.

## [0.1.0] — Foundation

### Added
- Initial workspace scaffold: `peterfan-core`, `peterfan-platform`,
  `peterfan-cli`, `peterfan-tui`.
- OS-agnostic core: temperature/fan/hardware types, validated fan curves with
  linear interpolation, and built-in profiles (Silent / Balanced / Gaming /
  Performance / Maximum / Custom).
- `HardwareProvider` trait with an up-front capability model.
- Mock backend: a fully simulated, controllable machine with drifting temps.
- macOS backend: real, read-only hardware info (CPU, memory, OS) via `sysctl`.
  Temperature/fan reading (SMC) is not yet implemented and reports
  `Unsupported`; the CLI/TUI fall back to simulated sensor data, clearly
  labeled.
- CLI (`peterfan`): `status`, `temps`, `fans`, `profile`, `curve`, `hardware`,
  `doctor`, with global `--mock` and `--json` flags.
- TUI (`peterfan-tui`): live ratatui dashboard with temperature/fan gauges and a
  CPU-temperature sparkline.
- Documentation: README, architecture, roadmap, CLI reference, contributing.

[Unreleased]: https://github.com/uulab-official/peterfan/compare/v0.27.1...HEAD
[0.27.1]: https://github.com/uulab-official/peterfan/releases/tag/v0.27.1
[0.27.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.27.0
[0.26.2]: https://github.com/uulab-official/peterfan/releases/tag/v0.26.2
[0.26.1]: https://github.com/uulab-official/peterfan/releases/tag/v0.26.1
[0.26.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.26.0
[0.25.2]: https://github.com/uulab-official/peterfan/releases/tag/v0.25.2
[0.25.1]: https://github.com/uulab-official/peterfan/releases/tag/v0.25.1
[0.25.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.25.0
[0.24.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.24.0
[0.23.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.23.0
[0.22.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.22.0
[0.21.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.21.0
[0.20.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.20.0
[0.19.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.19.0
[0.18.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.18.0
[0.17.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.17.0
[0.16.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.16.0
[0.15.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.15.0
[0.14.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.14.0
[0.13.2]: https://github.com/uulab-official/peterfan/releases/tag/v0.13.2
[0.13.1]: https://github.com/uulab-official/peterfan/releases/tag/v0.13.1
[0.13.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.13.0
[0.12.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.12.0
[0.11.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.11.0
[0.10.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.10.0
[0.9.1]: https://github.com/uulab-official/peterfan/releases/tag/v0.9.1
[0.9.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.9.0
[0.8.1]: https://github.com/uulab-official/peterfan/releases/tag/v0.8.1
[0.8.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.8.0
[0.7.1]: https://github.com/uulab-official/peterfan/releases/tag/v0.7.1
[0.7.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.7.0
[0.6.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.6.0
[0.5.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.5.0
[0.4.2]: https://github.com/uulab-official/peterfan/releases/tag/v0.4.2
[0.4.1]: https://github.com/uulab-official/peterfan/releases/tag/v0.4.1
[0.4.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.4.0
[0.3.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.3.0
[0.2.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.2.0
[0.1.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.1.0
