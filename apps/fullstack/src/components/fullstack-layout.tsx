import { AppSidebar } from "./app-sidebar"
import { RouteOutlet } from "../browser-primitives"

export function FullstackLayout() {
  return <div className="o1-app-layout">
    <AppSidebar />
    <RouteOutlet id="main" />
  </div>
}
