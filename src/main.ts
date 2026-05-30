import { mount } from "svelte";
import App from "./App.svelte";
import "github-markdown-css/github-markdown-light.css";
import "highlight.js/styles/github.css";
import "./app.css";

const target = document.getElementById("app");

if (!target) {
  throw new Error("App mount target was not found");
}

export default mount(App, { target });
