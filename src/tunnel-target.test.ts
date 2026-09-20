import { describe, expect, it } from "vitest";
import {
  buildContainerOptions,
  findContainer,
  resolveServerAfterRefresh,
  servicesFor,
  suggestNetwork,
  withErrorTooltip,
  type ComposeService,
} from "./tunnel-target";

const services: ComposeService[] = [
  {
    service: "db",
    container: "app-db-1",
    status: "Up 1 minute",
    ports: ["5432/tcp"],
    networks: ["shared", "app_default"],
    image: "postgres:16",
  },
  {
    service: "db",
    container: "app-db-2",
    status: "Up 1 minute",
    ports: ["5432/tcp"],
    networks: ["shared"],
    image: "postgres:16",
  },
];

describe("container tunnel targets", () => {
  it("builds distinct options for replicas", () => {
    expect(buildContainerOptions(services)).toEqual([
      { label: "db / app-db-1", value: "app-db-1" },
      { label: "db / app-db-2", value: "app-db-2" },
    ]);
  });

  it("finds the selected concrete container", () => {
    expect(findContainer(services, "app-db-2")?.container).toBe("app-db-2");
    expect(findContainer(services, "missing")).toBeNull();
  });

  it("prefers the project default network", () => {
    expect(suggestNetwork("app", services[0])).toBe("app_default");
  });

  it("selects the only attached network when no project default exists", () => {
    expect(
      suggestNetwork("app", {
        ...services[0],
        networks: ["shared"],
      }),
    ).toBe("shared");
  });

  it("leaves an ambiguous non-default network unselected", () => {
    expect(
      suggestNetwork("app", {
        ...services[0],
        networks: ["frontend", "backend"],
      }),
    ).toBe("");
  });
});

describe("cached service provenance", () => {
  it("exposes cached services to the server and project that produced them", () => {
    expect(servicesFor(services, { server: "s1", project: "web" }, "s1", "web")).toHaveLength(2);
  });

  it("hides services produced by another server with the same project name", () => {
    expect(servicesFor(services, { server: "s2", project: "web" }, "s1", "web")).toEqual([]);
  });

  it("hides services produced by another project on the same server", () => {
    expect(servicesFor(services, { server: "s1", project: "web" }, "s1", "api")).toEqual([]);
  });

  it("hides services with no recorded provenance", () => {
    expect(servicesFor(services, null, "s1", "web")).toEqual([]);
  });

  it("hides services until a server and project are chosen", () => {
    const source = { server: "s1", project: "web" };
    expect(servicesFor(services, source, "", "web")).toEqual([]);
    expect(servicesFor(services, source, "s1", "")).toEqual([]);
  });
});

describe("tunnel row tooltips", () => {
  it("appends the stored error to the remote label", () => {
    expect(
      withErrorTooltip(
        "staging / app / db (app-db-1):5432",
        "container app-db-1 is not running",
      ),
    ).toBe("staging / app / db (app-db-1):5432 — container app-db-1 is not running");
  });

  it("keeps the remote label when no error is stored", () => {
    expect(withErrorTooltip("staging / app / db (app-db-1):5432", null)).toBe(
      "staging / app / db (app-db-1):5432",
    );
    expect(withErrorTooltip("label", "")).toBe("label");
    expect(withErrorTooltip("label", "   ")).toBe("label");
  });
});

describe("server refresh validity", () => {
  it("keeps a server that still exists", () => {
    expect(resolveServerAfterRefresh("s2", ["s1", "s2"])).toBe("s2");
  });

  it("falls back to the first server when the selected one is gone", () => {
    expect(resolveServerAfterRefresh("gone", ["s1", "s2"])).toBe("s1");
  });

  it("clears the selection when no servers remain", () => {
    expect(resolveServerAfterRefresh("gone", [])).toBe("");
  });

  it("falls back to the first server when nothing was selected", () => {
    expect(resolveServerAfterRefresh("", ["s1"])).toBe("s1");
  });
});
