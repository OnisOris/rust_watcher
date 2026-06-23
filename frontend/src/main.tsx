
  import { createRoot } from "react-dom/client";
  import App from "./app/App.tsx";
  import "./app/canvasMiniMapGuard.ts";
  import "./styles/index.css";

  createRoot(document.getElementById("root")!).render(<App />);
  