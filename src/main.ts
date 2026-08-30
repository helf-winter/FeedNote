import { createApp } from "vue";
import App from "./App.vue";
import CaptureDot from "./surfaces/CaptureDot.vue";
import CaptureMenu from "./surfaces/CaptureMenu.vue";
import PlanDock from "./surfaces/PlanDock.vue";
import "./capture-dot.css";
import "./overlay.css";

const surface = new URLSearchParams(window.location.search).get("surface");
document.body.dataset.surface = surface ?? "main";

const component = surface === "capture-dot"
  ? CaptureDot
  : surface === "capture-menu"
    ? CaptureMenu
    : surface === "plan-dock"
      ? PlanDock
      : App;

if (surface) {
  createApp(component).mount("#app");
} else {
  void import("./styles.css").then(() => createApp(component).mount("#app"));
}
