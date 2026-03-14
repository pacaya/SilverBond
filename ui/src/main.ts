import { mount } from "svelte";
import App from "@/app/App.svelte";
import "@/styles.css";
import "@xyflow/svelte/dist/style.css";

mount(App, { target: document.getElementById("root")! });
