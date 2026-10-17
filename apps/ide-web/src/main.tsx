import React, { useEffect, useState, useSyncExternalStore } from 'react';
import ReactDOM from 'react-dom/client';
import { BrowserRouter, Navigate, Route, Routes } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { IdeApp, PlatformProvider, getPlatform } from '@nocode/ide-core';
import '@nocode/ide-core/styles.css';
import { webPlatform } from './platform/web';
import { Login } from './pages/Login';
import { Register } from './pages/Register';
import { Projects } from './pages/Projects';
import { bootstrapSession, getStatus, subscribe } from './auth/session';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 1000 * 60 * 5,
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
});

/**
 * Gate for the editor routes: requires a session and an open project. Falls
 * back to the project picker when the working copy is empty.
 */
function EditorGate() {
  const status = useSyncExternalStore(subscribe, getStatus, getStatus);
  const [hasProject, setHasProject] = useState<boolean | null>(null);

  useEffect(() => {
    let cancelled = false;
    void getPlatform()
      .getCurrentProject()
      .then((json) => {
        if (!cancelled) setHasProject(Boolean(json));
      })
      .catch(() => {
        if (!cancelled) setHasProject(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (status === 'unknown') {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-50 dark:bg-slate-900 text-gray-500">
        Loading…
      </div>
    );
  }
  if (status === 'anonymous') return <Navigate to="/login" replace />;
  if (hasProject === false) return <Navigate to="/projects" replace />;

  return <IdeApp />;
}

function Root() {
  const status = useSyncExternalStore(subscribe, getStatus, getStatus);

  return (
    <Routes>
      <Route
        path="/login"
        element={status === 'authenticated' ? <Navigate to="/projects" replace /> : <Login />}
      />
      <Route
        path="/register"
        element={status === 'authenticated' ? <Navigate to="/projects" replace /> : <Register />}
      />
      <Route
        path="/projects"
        element={status === 'authenticated' ? <Projects /> : <Navigate to="/login" replace />}
      />
      <Route path="/*" element={<EditorGate />} />
    </Routes>
  );
}

function App() {
  const [ready, setReady] = useState(false);

  useEffect(() => {
    void bootstrapSession().finally(() => setReady(true));
  }, []);

  return (
    <PlatformProvider platform={webPlatform}>
      <QueryClientProvider client={queryClient}>
        <BrowserRouter>{ready ? <Root /> : null}</BrowserRouter>
      </QueryClientProvider>
    </PlatformProvider>
  );
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);