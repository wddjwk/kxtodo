import { mount } from "svelte";
import App from "./App.svelte";
import "github-markdown-css/github-markdown-light.css";
import "./app.css";

const target = document.getElementById("app");

if (!target) {
  throw new Error("App mount target was not found");
}

function applyDpiAwareZoom(): void {
  const deviceScale = window.devicePixelRatio || 1;
  document.documentElement.style.setProperty("--dpi-zoom", String(1 / deviceScale));
}

applyDpiAwareZoom();
window.addEventListener("resize", applyDpiAwareZoom);

export default mount(App, { target });
