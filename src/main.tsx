import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Provider } from "react-redux";
import App from "./App";
import CaptureDot from "./surfaces/CaptureDot";
import CaptureMenu from "./surfaces/CaptureMenu";
import PlanDock from "./surfaces/PlanDock";
import { store } from "./store/store";
import "./capture-dot.css";
import "./overlay.css";

const surface = new URLSearchParams(window.location.search).get("surface");
document.body.dataset.surface = surface ?? "main";
const Component =
  surface === "capture-dot"
    ? CaptureDot
    : surface === "capture-menu"
      ? CaptureMenu
      : surface === "plan-dock"
        ? PlanDock
        : App;

if (!surface) void import("./styles.css");

createRoot(document.getElementById("app")!).render(
  <StrictMode>
    <Provider store={store}>
      <Component />
    </Provider>
  </StrictMode>,
);
