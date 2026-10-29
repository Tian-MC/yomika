import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { Providers } from "../App";
import { mockState, resetMockBackend } from "../lib/mock";
import { Cover } from "./Cover";

beforeEach(() => {
  resetMockBackend();
  mockState().app.vndb.blurSexualThreshold = 0;
});
afterEach(() => vi.restoreAllMocks());

it("keeps the original hidden until explicit reveal, including unsupported renderers", async () => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  const user = userEvent.setup();
  const { container } = render(<Providers><Cover title="Protected cover" metadata={{ coverPath: null, coverUrl: null, coverSexual: 2 }} src="/protected.jpg" revealable /></Providers>);
  const reveal = await screen.findByRole("button", { name: "Show cover" });
  const img = container.querySelector("img")!;
  expect(img).not.toBeVisible();
  Object.defineProperties(img, { naturalWidth: { value: 500 }, naturalHeight: { value: 700 } });
  vi.spyOn(img, "getBoundingClientRect").mockReturnValue({ width: 200, height: 280 } as DOMRect);
  fireEvent.load(img);
  expect(img).not.toBeVisible();
  expect(container.querySelector("canvas")).not.toBeVisible();
  await user.click(reveal);
  await waitFor(() => expect(container.querySelector("img")).toBeVisible());
  expect(container.querySelector("canvas")).not.toBeInTheDocument();
});

it("shows the blurred bitmap while keeping the source hidden", async () => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(() => ({ drawImage: vi.fn(), save: vi.fn(), restore: vi.fn(), fillRect: vi.fn() }) as unknown as CanvasRenderingContext2D);
  const { container } = render(<Providers><Cover title="Blurred cover" metadata={{ coverPath: null, coverUrl: null, coverSexual: 2 }} src="/blurred.jpg" revealable /></Providers>);
  await screen.findByRole("button", { name: "Show cover" });
  const img = container.querySelector("img")!;
  Object.defineProperties(img, { naturalWidth: { value: 500 }, naturalHeight: { value: 700 } });
  vi.spyOn(img, "getBoundingClientRect").mockReturnValue({ width: 200, height: 280 } as DOMRect);
  fireEvent.load(img);
  await waitFor(() => expect(container.querySelector("canvas")).toBeVisible());
  expect(img).not.toBeVisible();
});
