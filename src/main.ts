import { mount } from "svelte";
import App from "./App.svelte";
import "github-markdown-css/github-markdown-light.css";
import "highlight.js/styles/github.css";
import "./styles/base.css";
import "./styles/titlebar.css";
import "./styles/sidebar.css";
import "./styles/workspace.css";
import "./styles/settings.css";
import "./styles/menu.css";
import "./styles/shared.css";
import "./styles/editor.css";
import "./styles/mobile.css";

const target = document.getElementById("app");

if (!target) {
  throw new Error("App mount target was not found");
}

// app-shell 布局宽为 100vw/uiScale（大于视口），页面天然存在可滚空间；
// 输入法组合结束时浏览器对输入框 scrollIntoView 会把 html 滚出原位，
// 造成整个界面（含 fixed 菜单/弹窗）错位。页面本就不允许滚动，滚了立即复位。
window.addEventListener(
  "scroll",
  () => {
    if (window.scrollX !== 0 || window.scrollY !== 0) {
      window.scrollTo(0, 0);
    }
  },
  true
);

export default mount(App, { target });
