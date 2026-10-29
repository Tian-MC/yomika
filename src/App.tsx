import { useMemo, type ReactNode } from "react";
import { createHashRouter, RouterProvider, type RouteObject } from "react-router-dom";
import { AppLayout } from "./components/shell/Shell";
import { ErrorState } from "./components/ui/feedback";
import { I18nProvider, resolveUiLanguage, useT } from "./i18n";
import { DiagnosticsPage } from "./pages/diagnostics/DiagnosticsPage";
import { GameDetailPage } from "./pages/game/GameDetailPage";
import { ImportWizard } from "./pages/import/ImportWizard";
import { LibraryPage } from "./pages/library/LibraryPage";
import { OverlayPage } from "./pages/overlay/OverlayPage";
import { RuntimePage } from "./pages/runtime/RuntimePage";
import { SettingsPage } from "./pages/settings/SettingsPage";
import { TranslationPage } from "./pages/translation/TranslationPage";
import { AppStateProvider, useAppSettings } from "./state/app";
import { ToastProvider } from "./state/toasts";

function NotFound() {
  const t = useT();
  return (
    <div className="page">
      <ErrorState error={{ code: "notFound", message: t("nav.notFound"), details: null }} />
    </div>
  );
}

function RouteError() {
  const t = useT();
  return (
    <div className="page">
      <ErrorState error={{ code: "internal", message: t("error.render"), details: null }} onRetry={() => window.location.reload()} />
    </div>
  );
}

export const routes: RouteObject[] = [
  { path: "/overlay", element: <OverlayPage /> },
  {
    path: "/",
    element: <AppLayout />,
    errorElement: <RouteError />,
    children: [
      { index: true, element: <LibraryPage /> },
      { path: "game/:id", element: <GameDetailPage /> },
      { path: "import", element: <ImportWizard /> },
      { path: "translation", element: <TranslationPage /> },
      { path: "runtime", element: <RuntimePage /> },
      { path: "diagnostics", element: <DiagnosticsPage /> },
      { path: "settings", element: <SettingsPage /> },
      { path: "*", element: <NotFound /> },
    ],
  },
];

/** UI language follows AppSettings.uiLanguage, falling back to the system locale. */
function LanguageBridge({ children }: { children: ReactNode }) {
  const { appSettings } = useAppSettings();
  const lang = resolveUiLanguage(appSettings?.uiLanguage);
  return <I18nProvider lang={lang}>{children}</I18nProvider>;
}

export function Providers({ children }: { children: ReactNode }) {
  return (
    <ToastProvider>
      <AppStateProvider>
        <LanguageBridge>{children}</LanguageBridge>
      </AppStateProvider>
    </ToastProvider>
  );
}

export default function App() {
  const router = useMemo(() => createHashRouter(routes), []);
  return (
    <Providers>
      <RouterProvider router={router} />
    </Providers>
  );
}
