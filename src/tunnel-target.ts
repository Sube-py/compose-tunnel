export type ComposeService = {
  service: string;
  container: string;
  status: string;
  ports: string[];
  networks: string[];
  image: string;
};

export type ContainerOption = {
  label: string;
  value: string;
};

export function buildContainerOptions(services: ComposeService[]): ContainerOption[] {
  return services.map((service) => ({
    label: `${service.service} / ${service.container}`,
    value: service.container,
  }));
}

export function findContainer(
  services: ComposeService[],
  container: string,
): ComposeService | null {
  return services.find((service) => service.container === container) ?? null;
}

export function suggestNetwork(project: string, service: ComposeService): string {
  const projectDefault = `${project}_default`;
  if (service.networks.includes(projectDefault)) {
    return projectDefault;
  }
  return service.networks.length === 1 ? service.networks[0] : "";
}

export type ServiceSource = {
  server: string;
  project: string;
};

export function servicesFor(
  services: ComposeService[],
  source: ServiceSource | null,
  server: string,
  project: string,
): ComposeService[] {
  if (!source || !server || !project) {
    return [];
  }
  if (source.server !== server || source.project !== project) {
    return [];
  }
  return services;
}

export function resolveServerAfterRefresh(current: string, names: string[]): string {
  if (current && names.includes(current)) {
    return current;
  }
  return names[0] ?? "";
}
