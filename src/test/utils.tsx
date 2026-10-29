import { render } from "@testing-library/react";
import { createMemoryRouter, RouterProvider } from "react-router-dom";
import { Providers, routes } from "../App";

/** Render the full app (shell + routes) at `path`, backed by the mock backend. */
export function renderApp(path = "/") {
  const router = createMemoryRouter(routes, { initialEntries: [path] });
  const utils = render(
    <Providers>
      <RouterProvider router={router} />
    </Providers>,
  );
  return { ...utils, router };
}
