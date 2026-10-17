import React from 'react';
import ReactDOM from 'react-dom/client';
import { BrowserRouter } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { IdeApp, PlatformProvider } from '@nocode/ide-core';
import '@nocode/ide-core/styles.css';
import { webPlatform } from './platform/web';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 1000 * 60 * 5,
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
});

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <PlatformProvider platform={webPlatform}>
      <QueryClientProvider client={queryClient}>
        <BrowserRouter>
          <IdeApp />
        </BrowserRouter>
      </QueryClientProvider>
    </PlatformProvider>
  </React.StrictMode>,
);
