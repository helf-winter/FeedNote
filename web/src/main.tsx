import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { Provider, store } from "./store";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Provider store={store}>
      <App />
    </Provider>
  </React.StrictMode>,
);
