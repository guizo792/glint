import { mount } from "svelte";
import "./app.css";
import Launcher from "./Launcher.svelte";
import Settings from "./Settings.svelte";

const route = location.hash.slice(1);
const target = document.getElementById("app")!;

if (route === "settings" || route === "welcome") {
  mount(Settings, { target, props: { welcome: route === "welcome" } });
} else {
  mount(Launcher, { target });
}
