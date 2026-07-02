#!/usr/bin/env bash
# Double-click this if macOS refuses to open PeterFan with a "cannot verify
# developer" / "Apple could not verify..." warning. PeterFan isn't notarized
# (that requires a paid Apple Developer account), so a copy downloaded via a
# browser gets flagged by Gatekeeper — this just clears that flag directly,
# which is more reliable than the System Settings → Privacy & Security →
# "Open Anyway" path across different macOS versions.
#
# 이 파일을 더블클릭하면 "확인할 수 없는 개발자" 경고 없이 PeterFan을 열 수
# 있게 됩니다. 정식 인증서(유료 Apple Developer 계정) 없이 배포되는 앱이라
# 다운로드된 사본에 macOS가 보안 플래그를 붙이는데, 이 스크립트가 그 플래그를
# 직접 지워줍니다.
set -u

echo "Looking for PeterFan.app… / PeterFan.app을 찾는 중…"

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP=""
for candidate in "/Applications/PeterFan.app" "$DIR/PeterFan.app"; do
  if [[ -d "$candidate" ]]; then
    APP="$candidate"
    break
  fi
done

if [[ -z "$APP" ]]; then
  echo
  echo "Could not find PeterFan.app in /Applications or next to this script."
  echo "Drag PeterFan.app onto the Applications shortcut first, then try again."
  echo
  echo "PeterFan.app을 /Applications 폴더나 이 스크립트와 같은 위치에서 찾지"
  echo "못했습니다. 먼저 PeterFan.app을 Applications 폴더로 옮긴 후 다시"
  echo "실행해주세요."
  read -r -p "Press Enter to close / 아무 키나 눌러 종료… "
  exit 1
fi

echo "Found: $APP"
xattr -cr "$APP"

echo
echo "Done — PeterFan should now open normally. / 완료 — 이제 PeterFan을"
echo "정상적으로 열 수 있습니다."
read -r -p "Press Enter to close / 아무 키나 눌러 종료… "
