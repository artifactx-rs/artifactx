import {StrictMode} from 'react';
import {createRoot} from 'react-dom/client';
import {injectStyles} from '@a2ui/react/styles';
import App from './App';
import './index.css';

injectStyles();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
