// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { DEFAULT_SETTINGS } from "../../lib/ipc/types";
import { ServerTranscription } from "./ServerTranscription";

const fixture = vi.hoisted(() => ({
  mac: false,
  hasKey: vi.fn(),
  setKey: vi.fn(),
}));
vi.mock("../../state/hooks", () => {
  const backend = { kind: "tauri", hasTranscriptionApiKey: fixture.hasKey, setTranscriptionApiKey: fixture.setKey };
  return { useBackend: () => backend, useStatus: () => null };
});
vi.mock("../../lib/platform", () => ({ isMac: () => fixture.mac }));
vi.mock("../../components/ui/controls", () => ({
  Select: ({ label, value, options, onChange }: { label: string; value: string; options: { value: string; label: string }[]; onChange: (value: string) => void }) =>
    <select aria-label={label} value={value} onChange={(event) => onChange(event.target.value)}>{options.map((item) => <option key={item.value} value={item.value}>{item.label}</option>)}</select>,
}));

beforeEach(() => {
  fixture.mac = false;
  fixture.hasKey.mockReset().mockResolvedValue(false);
  fixture.setKey.mockReset().mockResolvedValue(undefined);
});
afterEach(cleanup);

it("preserves Windows key saving and enables uploads only after a key is saved", async () => {
  render(<ServerTranscription settings={DEFAULT_SETTINGS} save={vi.fn()} />);
  const enable = screen.getByRole("button", { name: "Enable OpenAI uploads" }) as HTMLButtonElement;
  expect(enable.disabled).toBe(true);
  fireEvent.change(screen.getByPlaceholderText("Paste your API key"), { target: { value: "sk-public-test-placeholder" } });
  fireEvent.click(screen.getByRole("button", { name: "Save API key" }));
  await waitFor(() => expect(fixture.setKey).toHaveBeenCalledWith("sk-public-test-placeholder"));
  await waitFor(() => expect(enable.disabled).toBe(false));
});

it("keeps Mac server/local choices available and prevents unsupported OpenAI key actions", async () => {
  fixture.mac = true;
  render(<ServerTranscription settings={DEFAULT_SETTINGS} save={vi.fn()} />);
  expect((screen.getByRole("button", { name: "Enable server uploads" }) as HTMLButtonElement).disabled).toBe(false);
  fireEvent.change(screen.getByLabelText("Transcription service"), { target: { value: "openai" } });
  expect((screen.getByPlaceholderText("API-key storage requires Windows") as HTMLInputElement).disabled).toBe(true);
  expect((screen.getByRole("button", { name: "Save API key" }) as HTMLButtonElement).disabled).toBe(true);
  expect((screen.getByRole("button", { name: "Enable OpenAI uploads" }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByText(/On a Mac, use local Metal processing/)).toBeTruthy();
  expect(fixture.hasKey).not.toHaveBeenCalled();
  expect(fixture.setKey).not.toHaveBeenCalled();
});
