import { RouteDefinition } from "@lib";
import { Navigate, Route, Router, type RouteSectionProps } from "@solidjs/router";
import { type Component, For } from "solid-js";
import "./App.scss";
import { TopBar } from "./components/TopBar.tsx";
import { Toasts } from "./components/Toasts.tsx";
import { NowRoute } from "./routes/now.tsx";
import { PlaylistsRoute } from "./routes/playlists.tsx";
import { GridRoute } from "./routes/grid.tsx";
import { WifiRoute } from "./routes/wifi.tsx";
import { SetupRoute } from "./routes/setup.tsx";

const Routes: readonly RouteDefinition[] = [
  {
    label: "Now",
    path: "/",
    component: NowRoute,
  },
  {
    label: "Playlists",
    path: "/playlists",
    component: PlaylistsRoute,
  },
  {
    label: "Grid",
    path: "/grid",
    component: GridRoute,
  },
  {
    label: "WiFi",
    path: "/wifi",
    component: WifiRoute,
  },
  {
    label: "Setup",
    path: "/setup",
    component: SetupRoute,
  },
];

export function App() {
  const root: Component<RouteSectionProps> = (props) => (
    <div class="shell">
      <TopBar routes={Routes} pathname={props.location.pathname} />
      <main class="content">{props.children}</main>
      <Toasts />
    </div>
  );

  return (
    <Router root={root}>
      <For each={Routes}>
        {(page) => <Route path={page.path} component={page.component} />}
      </For>

      {/* Old paths from the previous UI / bookmarked festival crews. */}
      <Route path="/remote" component={() => <Navigate href="/playlists" />} />
      <Route path="/config" component={() => <Navigate href="/setup" />} />

      <Route path="*404" component={() => <Navigate href="/" />} />
    </Router>
  );
}
