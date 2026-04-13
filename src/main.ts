import App from "./App.svelte";
import { mount } from "svelte";
import { devWarn } from "$lib/utils/devLogger";

try {
  const savedTheme = localStorage.getItem("carrytalk.theme");

  if (savedTheme === "light") {
    document.documentElement.setAttribute("data-theme", "light");
  } else {
    document.documentElement.removeAttribute("data-theme");
  }
} catch (error) {
  devWarn("Failed to preload theme:", error);
}

const app = mount(App, { target: document.getElementById("app")! });

export default app;
