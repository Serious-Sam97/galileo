import { describe, it, expect } from "vitest";
import { callSite } from "../src/callsite.js";
import { identityAttributes, runWithIdentity, currentIdentity, setIdentity } from "../src/identity.js";

describe("callSite", () => {
  it("finds the app frame and skips the sdk", () => {
    function loadUsers() { return callSite(process.cwd(), [/vitest/, /node_modules/]); }
    const cs = loadUsers();
    expect(cs).toBeDefined();
    expect(cs!["code.function.name"]).toBe("loadUsers");
    expect(cs!["code.file.path"]).toMatch(/callsite\.test\.ts$/);
    expect(cs!["code.line.number"]).toBeGreaterThan(0);
  });
});

describe("identity", () => {
  it("propagates through async context and maps to user.* attributes", async () => {
    await runWithIdentity({ id: 42, email: "a@b.c", tenant: "acme" }, async () => {
      await new Promise((r) => setTimeout(r, 1));
      expect(currentIdentity()?.id).toBe(42);
      setIdentity({ name: "Ann" });
      expect(identityAttributes(currentIdentity())).toEqual({ "user.id": "42", "user.email": "a@b.c", "user.name": "Ann", "tenant.id": "acme" });
    });
    expect(currentIdentity()).toBeUndefined();
  });
});
