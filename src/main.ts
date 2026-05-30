import { mount } from "svelte";
import App from "./App.svelte";
import "github-markdown-css/github-markdown-light.css";
import "highlight.js/styles/github.css";
import "./styles/base.css";
import "./styles/titlebar.css";
import "./styles/sidebar.css";
import "./styles/workspace.css";
import "./styles/settings.css";
import "./styles/shared.css";

const target = document.getElementById("app");

if (!target) {
  throw new Error("App mount target was not found");
}

export default mount(App, { target });
