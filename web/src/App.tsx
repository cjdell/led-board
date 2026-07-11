import { RouteDefinition } from "@lib";
import { Route, Router, type RouteSectionProps, useNavigate } from "@solidjs/router";
import { type Component, For, Suspense } from "solid-js";
import "./App.scss";
import { NavBar } from "./components/NavBar/index.tsx";
import { ConfigRoute } from "./routes/config.tsx";
import { FilesRoute } from "./routes/files.tsx";
import { IndexRoute } from "./routes/index.tsx";
import { RemoteRoute } from "./routes/remote.tsx";
import { WifiRoute } from "./routes/wifi.tsx";
import "./sass/bootstrap.scss";

const Routes: readonly RouteDefinition[] = [
  {
    label: "Home",
    path: "/",
    component: IndexRoute,
  },
  {
    label: "Remote",
    path: "/remote",
    component: RemoteRoute,
  },
  {
    label: "Files",
    path: "/fs",
    component: FilesRoute,
  },
  {
    label: "Config",
    path: "/config",
    component: ConfigRoute,
  },
  {
    label: "WiFi",
    path: "/wifi",
    component: WifiRoute,
  },
];

export function App() {
  const root: Component<RouteSectionProps> = (props) => (
    <>
      <NavBar routes={Routes} pathname={props.location.pathname} />
      <main class="container">
        <Suspense>{props.children}</Suspense>
      </main>
    </>
  );

  return (
    <Router root={root}>
      <For each={Routes}>
        {(page) => <Route path={page.path} component={page.component} />}
      </For>
      <Route path="*404" component={NotFound} />
    </Router>
  );
}

function NotFound() {
  const navigate = useNavigate();

  navigate("/");

  return <div>Redirecting...</div>;
}
