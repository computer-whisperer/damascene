import Slicer from "./views/slicer"
import Voice from "./views/voice"
import Parts from "./views/parts"
import Settings from "./views/settings"

/**
 * `?view=` router, mirroring references/shadcn-calibration. Each view
 * is one reference app at exactly 1280x800.
 */
const VIEWS: Record<string, () => React.ReactNode> = {
  slicer: () => <Slicer />,
  voice: () => <Voice />,
  parts: () => <Parts />,
  settings: () => <Settings />,
}

export default function App() {
  const view = new URLSearchParams(window.location.search).get("view") ?? "slicer"
  const render = VIEWS[view]
  return (
    <div className="h-[800px] w-[1280px] overflow-hidden">
      {render ? render() : <div className="p-8">unknown view: {view}</div>}
    </div>
  )
}
