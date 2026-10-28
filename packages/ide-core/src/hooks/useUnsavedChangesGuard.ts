/**
 * Guard against losing unsaved work.
 *
 * The IDE tracks dirtiness (`projectStore.isDirty`) and displays it, but
 * nothing acted on it: closing the tab silently discarded edits, signing out
 * navigated away with no check, and opening another project replaced the
 * working copy in place.
 *
 * WHY NOT `useBlocker`: it is the right tool, but it throws unless the app is
 * mounted under a *data router* (`createBrowserRouter`). Both hosts mount
 * `BrowserRouter` with `<Routes>`, so calling it here would break the IDE
 * rather than protect it. The guards below cover the three ways work is
 * actually lost — tab close, sign-out, and opening another project — without
 * forcing a router migration into this change.
 */
import { useEffect } from 'react';
import { useProjectStore } from '../stores/projectStore';

/**
 * Ask before discarding unsaved changes. Returns `true` when it is safe to
 * proceed (nothing dirty, or the user confirmed).
 *
 * Call this at the start of any handler that would replace or discard the
 * working copy.
 */
export function confirmLeave(): boolean {
  if (!useProjectStore.getState().isDirty) return true;
  return window.confirm(
    'This project has unsaved changes. Leave without saving them?'
  );
}

/**
 * Warn on tab close / reload while the project is dirty.
 *
 * Mount once, high in the editor tree. The listener is only attached while
 * something is actually unsaved, so a clean project closes silently.
 */
export function useUnsavedChangesGuard(): { isDirty: boolean } {
  const isDirty = useProjectStore((state) => state.isDirty);

  useEffect(() => {
    if (!isDirty) return;
    const handler = (event: BeforeUnloadEvent) => {
      // Both are required: `preventDefault` is the standard, `returnValue` is
      // what Chrome and older Firefox look at.
      event.preventDefault();
      event.returnValue = '';
    };
    window.addEventListener('beforeunload', handler);
    return () => window.removeEventListener('beforeunload', handler);
  }, [isDirty]);

  return { isDirty };
}
