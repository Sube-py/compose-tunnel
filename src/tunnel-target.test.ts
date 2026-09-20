import { describe, expect, it } from "vitest";
import {
  buildContainerOptions,
  findContainer,
  suggestNetwork,
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

  it("leaves an ambiguous non-default network unselected", () => {
    expect(
      suggestNetwork("app", {
        ...services[0],
        networks: ["frontend", "backend"],
      }),
    ).toBe("");
  });
});
