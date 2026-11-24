# 데스크톱 UI

React 화면과 모달은 [HeroUI v3](https://heroui.com/en/docs/react/getting-started)의 Card, Button, Select, Table, Tabs, Modal, Switch, Slider, ProgressBar 및 피드백 컴포넌트로 구성합니다. Figma의 시작 화면과 렌더링 목록에서 가져온 색상, 글꼴, 간격, 모서리 토큰을 `ui/src/styles.css`에서 관리하고 모니터, 연결, 설정, 작업 기록, 대기열 관리에도 공유합니다. 원본 프레임의 배치는 `ui/src/reference.css`에 있습니다. 글꼴은 Wanted Sans이며, Figma에서 내보낸 아이콘은 같은 동작과 상태에 재사용합니다. 추가 동작에는 Lucide 아이콘을 사용합니다.

Cargo 빌드는 잠긴 npm 의존성으로 화면을 빌드하고 생성된 정적 자산을 실행 파일에 포함합니다. Wry가 운영체제의 WebView에서 자산을 열고, Rust와의 메시지 통신으로 대기열과 렌더 상태를 연결합니다. 앱 UI를 제공하기 위한 로컬 HTTP 서버나 CDN 의존성은 없습니다.

HeroUI는 Apache-2.0, [Lucide](https://lucide.dev/license)는 ISC, [Wanted Sans](../assets/ui/OFL.txt)는 SIL OFL 1.1을 따릅니다.
