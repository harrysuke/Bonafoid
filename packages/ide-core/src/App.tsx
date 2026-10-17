import { useEffect } from 'react';
import { Routes, Route, Navigate } from 'react-router-dom';
import { Layout } from './components/Layout';
import { Dashboard } from './pages/Dashboard';
import { Settings } from './pages/Settings';
import { FormBuilder } from './pages/FormBuilder';
import { WorkflowBuilder } from './pages/WorkflowBuilder';
import { DataModeler } from './pages/DataModeler';
import { ListBuilder } from './pages/ListBuilder';
import { PageBuilder } from './pages/PageBuilder';
import { ThemeBuilder } from './pages/ThemeBuilder';
import { PluginManager } from './pages/PluginManager';
import { CodeGenerator } from './pages/CodeGenerator';
import { PreviewPanel } from './pages/PreviewPanel';
import { useProjectStore } from './stores/projectStore';

export function IdeApp() {
  const init = useProjectStore((s) => s.init);

  useEffect(() => {
    void init();
  }, [init]);

  return (
    <Routes>
      <Route element={<Layout />}>
        <Route path="/" element={<Dashboard />} />
        <Route path="/forms" element={<FormBuilder />} />
        <Route path="/forms/:id" element={<FormBuilder />} />
        <Route path="/workflows" element={<WorkflowBuilder />} />
        <Route path="/workflows/:id" element={<WorkflowBuilder />} />
        <Route path="/data" element={<DataModeler />} />
        <Route path="/data/:id" element={<DataModeler />} />
        <Route path="/lists" element={<ListBuilder />} />
        <Route path="/lists/:id" element={<ListBuilder />} />
        <Route path="/pages" element={<PageBuilder />} />
        <Route path="/pages/:id" element={<PageBuilder />} />
        <Route path="/theme" element={<ThemeBuilder />} />
        <Route path="/plugins" element={<PluginManager />} />
        <Route path="/preview" element={<PreviewPanel />} />
        <Route path="/generate" element={<CodeGenerator />} />
        <Route path="/settings" element={<Settings />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  );
}

export default IdeApp;
