import React from 'react';
import { createRoot } from 'react-dom/client';
import App from './app/App';
import './shared/styles.css';
import { configureEchoPort } from './modules/echo/tauri';

configureEchoPort();

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
