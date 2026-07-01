<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useToast } from "primevue/usetoast";
import Button from "primevue/button";
import Card from "primevue/card";
import Column from "primevue/column";
import DataTable from "primevue/datatable";
import InputNumber from "primevue/inputnumber";
import InputText from "primevue/inputtext";
import ScrollPanel from "primevue/scrollpanel";
import Select from "primevue/select";
import Tag from "primevue/tag";
import Toast from "primevue/toast";

type Defaults = {
  local_host: string;
  socat_image: string;
  socat_command: string;
  ssh_binary: string;
  docker_timeout_secs: number;
};

type ServerConfig = {
  name: string;
  host: string;
  port: number;
  user: string;
  identity_file?: string | null;
  ssh_alias?: string | null;
  default_socat_image?: string | null;
  docker_command: string;
};

type ComposeProject = {
  server: string;
  project: string;
  services: string[];
};

type ComposeService = {
  service: string;
  container: string;
  status: string;
  ports: string[];
  networks: string[];
  image: string;
};

type TunnelState = {
  id: string;
  server: string;
  project: string;
  service: string;
  network: string;
  target_port: number;
  socat_port: number;
  local_host: string;
  local_port: number;
  socat_container: string;
  socat_container_ip: string;
  ssh_pid?: number | null;
  status: "running" | "stopped" | "error";
  mode: "socat-direct";
  env_prefix?: string | null;
  started_at?: string | null;
  last_error?: string | null;
};

const tabs = ["Dashboard", "Servers", "Compose", "Tunnels", "Env", "Logs", "Settings"] as const;
type Tab = (typeof tabs)[number];

const toast = useToast();
const activeTab = ref<Tab>("Dashboard");
const loading = ref(false);
const notice = ref("");
const error = ref("");
const logs = ref<string[]>([]);
const configDir = ref("");
const defaults = reactive<Defaults>({
  local_host: "127.0.0.1",
  socat_image: "alpine/socat:latest",
  socat_command: "socat",
  ssh_binary: "ssh",
  docker_timeout_secs: 20,
});
const servers = ref<ServerConfig[]>([]);
const projects = ref<ComposeProject[]>([]);
const services = ref<ComposeService[]>([]);
const tunnels = ref<TunnelState[]>([]);
const selectedServer = ref("");
const selectedProject = ref("");
const selectedTunnel = ref("");
const envPreview = ref("");
const envPath = ref(".env.local");
const editingServerName = ref("");

const serverForm = reactive<ServerConfig>({
  name: "",
  host: "",
  port: 22,
  user: "",
  identity_file: "",
  ssh_alias: "",
  default_socat_image: "",
  docker_command: "docker",
});

const tunnelForm = reactive({
  server: "",
  project: "",
  service: "",
  target_port: 5432,
  network: "",
  local_port: "",
  env_prefix: "DATABASE",
  socat_image: "",
});

const runningCount = computed(() => tunnels.value.filter((item) => item.status === "running").length);
const stoppedCount = computed(() => tunnels.value.filter((item) => item.status !== "running").length);
const dockerCommandPreset = computed(() => {
  if (serverForm.docker_command === "docker") {
    return "docker";
  }
  if (serverForm.docker_command === "sudo -n docker") {
    return "sudo -n docker";
  }
  return "custom";
});
const dockerModeOptions = [
  { label: "docker", value: "docker" },
  { label: "sudo -n docker", value: "sudo -n docker" },
  { label: "custom", value: "custom" },
];
const serverOptions = computed(() => servers.value.map((server) => ({ label: server.name, value: server.name })));
const tunnelOptions = computed(() => tunnels.value.map((tunnel) => ({ label: tunnel.id, value: tunnel.id })));
const tunnelProjectOptions = computed(() =>
  projects.value.filter((project) => project.server === tunnelForm.server),
);
const tunnelServiceOptions = computed(() =>
  services.value.filter(() => selectedServer.value === tunnelForm.server && selectedProject.value === tunnelForm.project),
);
const selectedTunnelService = computed(
  () => tunnelServiceOptions.value.find((service) => service.service === tunnelForm.service) ?? null,
);

function setTab(tab: Tab) {
  activeTab.value = tab;
  error.value = "";
  notice.value = "";
}

function log(message: string) {
  logs.value.unshift(`${new Date().toLocaleTimeString()} ${message}`);
  logs.value = logs.value.slice(0, 80);
}

async function runTask<T>(message: string, task: () => Promise<T>): Promise<T | null> {
  loading.value = true;
  error.value = "";
  notice.value = "";
  try {
    const result = await task();
    notice.value = message;
    log(message);
    return result;
  } catch (caught) {
    const text = caught instanceof Error ? caught.message : String(caught);
    error.value = text;
    log(`Error: ${text}`);
    toast.add({ severity: "error", summary: "Operation failed", detail: text, life: 5000 });
    return null;
  } finally {
    loading.value = false;
  }
}

async function bootstrap() {
  await runTask("Workspace loaded", async () => {
    const paths = await invoke<{ config_dir: string }>("init_config");
    configDir.value = paths.config_dir;
    const config = await invoke<{ defaults: Defaults }>("get_config");
    Object.assign(defaults, config.defaults);
    await refreshServers();
    await refreshTunnels();
  });
}

async function refreshServers() {
  servers.value = await invoke<ServerConfig[]>("list_servers");
  if (!selectedServer.value && servers.value.length > 0) {
    selectedServer.value = servers.value[0].name;
  }
  if (!tunnelForm.server && servers.value.length > 0) {
    tunnelForm.server = servers.value[0].name;
  }
}

async function refreshTunnels() {
  tunnels.value = await invoke<TunnelState[]>("list_tunnels");
  if (!selectedTunnel.value && tunnels.value.length > 0) {
    selectedTunnel.value = tunnels.value[0].id;
  }
}

async function saveServer() {
  await runTask(`Saved server ${serverForm.name}`, async () => {
    const server = compactServer(serverForm);
    if (editingServerName.value && editingServerName.value !== server.name) {
      await invoke("delete_server", { name: editingServerName.value });
    }
    await invoke("save_server", { server });
    clearServerForm();
    await refreshServers();
  });
}

async function deleteServer(name: string) {
  await runTask(`Deleted server ${name}`, async () => {
    await invoke("delete_server", { name });
    await refreshServers();
  });
}

async function testServer(name: string) {
  const result = await runTask(`Tested server ${name}`, async () =>
    invoke<{ details: string[] }>("test_server", { serverId: name }),
  );
  if (result) {
    logs.value.unshift(...result.details.map((detail) => `${new Date().toLocaleTimeString()} ${detail}`));
    const ok = result.details.length > 0 && !result.details.some((detail) => detail.toLowerCase().includes("failed"));
    toast.add({
      severity: ok ? "success" : "error",
      summary: ok ? "Connection test passed" : "Connection test failed",
      detail: result.details.join(" | "),
      life: ok ? 3500 : 7000,
    });
  }
}

function editServer(server: ServerConfig) {
  editingServerName.value = server.name;
  Object.assign(serverForm, {
    name: server.name,
    host: server.host,
    port: server.port,
    user: server.user,
    identity_file: server.identity_file ?? "",
    ssh_alias: server.ssh_alias ?? "",
    default_socat_image: server.default_socat_image ?? "",
    docker_command: server.docker_command || "docker",
  });
  notice.value = `Editing server ${server.name}`;
  error.value = "";
}

function applyDockerCommandPreset(value: string) {
  if (value === "docker" || value === "sudo -n docker") {
    serverForm.docker_command = value;
  }
}

async function discoverProjects() {
  if (!selectedServer.value) {
    error.value = "Select a server first";
    return;
  }
  const result = await runTask(`Discovered projects on ${selectedServer.value}`, async () =>
    invoke<ComposeProject[]>("list_compose_projects", { serverId: selectedServer.value }),
  );
  if (result) {
    projects.value = result;
    services.value = [];
    selectedProject.value = "";
  }
}

async function loadServices(project: string) {
  selectedProject.value = project;
  const result = await runTask(`Loaded services for ${project}`, async () =>
    invoke<ComposeService[]>("list_compose_services", {
      serverId: selectedServer.value,
      project,
    }),
  );
  if (result) {
    services.value = result;
  }
}

function pickService(service: ComposeService) {
  tunnelForm.server = selectedServer.value;
  tunnelForm.project = selectedProject.value;
  tunnelForm.service = service.service;
  tunnelForm.network = service.networks[0] ?? "";
  tunnelForm.local_port = "";
  const port = inferPort(service);
  if (port) {
    tunnelForm.target_port = port;
  }
  activeTab.value = "Tunnels";
}

async function openTunnel() {
  await runTask(`Opened tunnel for ${tunnelForm.service}`, async () => {
    await invoke("open_tunnel", {
      request: {
        server: tunnelForm.server,
        project: tunnelForm.project,
        service: tunnelForm.service,
        target_port: Number(tunnelForm.target_port),
        network: optional(tunnelForm.network),
        local_port: tunnelForm.local_port ? Number(tunnelForm.local_port) : null,
        local_host: defaults.local_host,
        socat_port: null,
        socat_image: optional(tunnelForm.socat_image),
        env_prefix: optional(tunnelForm.env_prefix),
      },
    });
    tunnelForm.local_port = "";
    await refreshTunnels();
  });
}

async function onTunnelServerChange() {
  selectedServer.value = tunnelForm.server;
  tunnelForm.project = "";
  tunnelForm.service = "";
  tunnelForm.network = "";
  services.value = [];
  if (tunnelForm.server) {
    const result = await runTask(`Discovered projects on ${tunnelForm.server}`, async () =>
      invoke<ComposeProject[]>("list_compose_projects", { serverId: tunnelForm.server }),
    );
    if (result) {
      projects.value = result;
    }
  }
}

async function onTunnelProjectChange() {
  selectedServer.value = tunnelForm.server;
  selectedProject.value = tunnelForm.project;
  tunnelForm.service = "";
  tunnelForm.network = "";
  if (tunnelForm.server && tunnelForm.project) {
    await loadServices(tunnelForm.project);
  }
}

function onTunnelServiceChange() {
  const service = selectedTunnelService.value;
  if (!service) {
    tunnelForm.network = "";
    return;
  }
  tunnelForm.network = service.networks[0] ?? "";
  const port = inferPort(service);
  if (port) {
    tunnelForm.target_port = port;
  }
}

async function closeTunnel(id: string) {
  await runTask(`Stopped tunnel ${id}`, async () => {
    await invoke("close_tunnel", { tunnelId: id });
    await refreshTunnels();
  });
}

async function startTunnel(tunnel: TunnelState) {
  await runTask(`Started tunnel ${tunnel.id}`, async () => {
    await invoke("open_tunnel", {
      request: {
        server: tunnel.server,
        project: tunnel.project,
        service: tunnel.service,
        target_port: tunnel.target_port,
        network: tunnel.network,
        local_port: null,
        local_host: tunnel.local_host || defaults.local_host,
        socat_port: tunnel.socat_port,
        socat_image: null,
        env_prefix: tunnel.env_prefix,
      },
    });
    await refreshTunnels();
  });
}

async function renderEnv() {
  if (!selectedTunnel.value) {
    error.value = "Select a tunnel first";
    return;
  }
  const result = await runTask(`Rendered env for ${selectedTunnel.value}`, async () =>
    invoke<string>("render_env", { tunnelId: selectedTunnel.value }),
  );
  if (result !== null) {
    envPreview.value = result;
  }
}

async function writeEnv() {
  if (!selectedTunnel.value) {
    error.value = "Select a tunnel first";
    return;
  }
  await runTask(`Wrote env block to ${envPath.value}`, async () => {
    await invoke("write_env_file", {
      request: {
        tunnel_id: selectedTunnel.value,
        path: envPath.value,
      },
    });
  });
}

async function saveDefaultSettings() {
  await runTask("Saved settings", async () => {
    await invoke("save_defaults", { defaults: { ...defaults } });
  });
}

function clearServerForm() {
  editingServerName.value = "";
  Object.assign(serverForm, {
    name: "",
    host: "",
    port: 22,
    user: "",
    identity_file: "",
    ssh_alias: "",
    default_socat_image: "",
    docker_command: "docker",
  });
}

function compactServer(server: ServerConfig): ServerConfig {
  return {
    name: server.name.trim(),
    host: server.host.trim(),
    port: Number(server.port),
    user: server.user.trim(),
    identity_file: optional(server.identity_file),
    ssh_alias: optional(server.ssh_alias),
    default_socat_image: optional(server.default_socat_image),
    docker_command: server.docker_command.trim() || "docker",
  };
}

function optional(value?: string | null) {
  const trimmed = value?.trim();
  return trimmed ? trimmed : null;
}

function inferPort(service: ComposeService) {
  const text = service.ports.join(" ");
  const match = text.match(/(\d+)\/tcp/) ?? text.match(/:(\d+)->/);
  return match ? Number(match[1]) : null;
}

function statusSeverity(status: string) {
  if (status === "running") {
    return "success";
  }
  if (status === "error") {
    return "danger";
  }
  return "secondary";
}

function onDockerModeChange(value: string) {
  applyDockerCommandPreset(value);
}

onMounted(bootstrap);
</script>

<template>
  <main class="shell">
    <Toast position="top-right" />
    <aside class="sidebar">
      <div class="brand">
        <span class="brand-mark">CT</span>
        <div>
          <h1>Compose Tunnel</h1>
          <p>SSH to Compose internals</p>
        </div>
      </div>
      <nav>
        <Button
          v-for="tab in tabs"
          :key="tab"
          :label="tab"
          :class="{ active: activeTab === tab }"
          type="button"
          text
          @click="setTab(tab)"
        />
      </nav>
      <div class="config-path">{{ configDir }}</div>
    </aside>

    <ScrollPanel class="workspace-scroll">
      <section class="workspace">
        <header class="topbar">
          <div>
            <h2>{{ activeTab }}</h2>
            <p>{{ runningCount }} running tunnels, {{ stoppedCount }} stopped</p>
          </div>
          <Button label="Refresh" icon="pi pi-refresh" :loading="loading" @click="refreshTunnels" />
        </header>

      <div v-if="notice" class="notice">{{ notice }}</div>
      <div v-if="error" class="error">{{ error }}</div>

      <section v-if="activeTab === 'Dashboard'" class="page">
        <div class="metrics">
          <Card>
            <template #content><span>Running</span><strong>{{ runningCount }}</strong></template>
          </Card>
          <Card>
            <template #content><span>Servers</span><strong>{{ servers.length }}</strong></template>
          </Card>
          <Card>
            <template #content><span>Projects</span><strong>{{ projects.length }}</strong></template>
          </Card>
        </div>
        <div class="toolbar">
          <Button label="Add Server" icon="pi pi-server" outlined @click="setTab('Servers')" />
          <Button label="Discover Compose" icon="pi pi-search" outlined @click="setTab('Compose')" />
          <Button label="Open Tunnel" icon="pi pi-share-alt" outlined @click="setTab('Tunnels')" />
        </div>
        <DataTable :value="tunnels" size="small" stripedRows>
          <Column field="id" header="ID" />
          <Column header="Service">
            <template #body="{ data }">{{ data.project }}/{{ data.service }}:{{ data.target_port }}</template>
          </Column>
          <Column header="Local">
            <template #body="{ data }">{{ data.local_host }}:{{ data.local_port }}</template>
          </Column>
          <Column header="Status">
            <template #body="{ data }"><Tag :value="data.status" :severity="statusSeverity(data.status)" /></template>
          </Column>
        </DataTable>
      </section>

      <section v-if="activeTab === 'Servers'" class="page split">
        <form class="panel form" @submit.prevent="saveServer">
          <h3>{{ editingServerName ? `Edit ${editingServerName}` : 'Server' }}</h3>
          <label>Name<InputText v-model="serverForm.name" required /></label>
          <label>Host<InputText v-model="serverForm.host" required /></label>
          <label>User<InputText v-model="serverForm.user" required /></label>
          <label>Port<InputNumber v-model="serverForm.port" :min="1" :useGrouping="false" fluid /></label>
          <label>Identity file<InputText v-model="serverForm.identity_file" placeholder="~/.ssh/id_ed25519" /></label>
          <label>SSH config alias<InputText v-model="serverForm.ssh_alias" placeholder="staging" /></label>
          <label>
            Docker mode
            <Select :modelValue="dockerCommandPreset" :options="dockerModeOptions" optionLabel="label" optionValue="value" @update:modelValue="onDockerModeChange" />
          </label>
          <label>Docker command<InputText v-model="serverForm.docker_command" required /></label>
          <label>Default socat image<InputText v-model="serverForm.default_socat_image" placeholder="alpine/socat:latest" /></label>
          <div class="toolbar">
            <Button :label="editingServerName ? 'Update' : 'Save'" icon="pi pi-save" type="submit" />
            <Button :label="editingServerName ? 'Cancel' : 'Clear'" severity="secondary" outlined type="button" @click="clearServerForm" />
          </div>
        </form>
        <div class="panel">
          <h3>Servers</h3>
          <DataTable :value="servers" size="small" stripedRows>
            <Column field="name" header="Name" />
            <Column header="Host">
              <template #body="{ data }">{{ data.host }}:{{ data.port }}</template>
            </Column>
            <Column field="user" header="User" />
            <Column field="docker_command" header="Docker" />
            <Column header="Actions">
              <template #body="{ data }">
                <div class="actions">
                  <Button icon="pi pi-pencil" label="Edit" size="small" text @click="editServer(data)" />
                  <Button icon="pi pi-check-circle" label="Test" size="small" text @click="testServer(data.name)" />
                  <Button icon="pi pi-trash" label="Delete" size="small" severity="danger" text @click="deleteServer(data.name)" />
                </div>
              </template>
            </Column>
          </DataTable>
        </div>
      </section>

      <section v-if="activeTab === 'Compose'" class="page">
        <div class="toolbar">
          <Select v-model="selectedServer" :options="serverOptions" optionLabel="label" optionValue="value" placeholder="Select server" />
          <Button label="Discover" icon="pi pi-search" :loading="loading" @click="discoverProjects" />
        </div>
        <div class="project-grid">
          <article v-for="project in projects" :key="project.project" class="panel">
            <div class="project-card-header">
              <h3>{{ project.project }}</h3>
              <Button label="Services" icon="pi pi-list" size="small" outlined @click="loadServices(project.project)" />
            </div>
            <p>{{ project.services.join(', ') }}</p>
          </article>
        </div>
        <DataTable :value="services" size="small" stripedRows>
          <Column field="service" header="Service" />
          <Column field="container" header="Container" />
          <Column field="status" header="Status" />
          <Column header="Ports"><template #body="{ data }">{{ data.ports.join(', ') }}</template></Column>
          <Column header="Networks"><template #body="{ data }">{{ data.networks.join(', ') }}</template></Column>
          <Column header=""><template #body="{ data }"><Button label="Tunnel" icon="pi pi-share-alt" size="small" @click="pickService(data)" /></template></Column>
        </DataTable>
      </section>

      <section v-if="activeTab === 'Tunnels'" class="page split">
        <form class="panel form" @submit.prevent="openTunnel">
          <h3>Create Tunnel</h3>
          <label>
            Server
            <Select v-model="tunnelForm.server" :options="serverOptions" optionLabel="label" optionValue="value" placeholder="Select server" @update:modelValue="onTunnelServerChange" />
          </label>
          <label>
            Project
            <Select v-model="tunnelForm.project" :options="tunnelProjectOptions" optionLabel="project" optionValue="project" placeholder="Select project" @update:modelValue="onTunnelProjectChange" />
          </label>
          <label>
            Service container
            <Select v-model="tunnelForm.service" :options="tunnelServiceOptions" optionLabel="container" optionValue="service" placeholder="Select service" @update:modelValue="onTunnelServiceChange" />
          </label>
          <label>
            Network
            <Select v-if="selectedTunnelService" v-model="tunnelForm.network" :options="selectedTunnelService.networks" placeholder="Select network" />
            <InputText v-else v-model="tunnelForm.network" placeholder="myapp_default" />
          </label>
          <label>Target port<InputNumber v-model="tunnelForm.target_port" :min="1" :useGrouping="false" fluid /></label>
          <label>Local port<InputText v-model="tunnelForm.local_port" placeholder="auto assign" /></label>
          <label>Env prefix<InputText v-model="tunnelForm.env_prefix" placeholder="DATABASE" /></label>
          <label>socat image<InputText v-model="tunnelForm.socat_image" :placeholder="defaults.socat_image" /></label>
          <Button label="Start" icon="pi pi-play" type="submit" />
        </form>
        <div class="panel">
          <h3>Tunnels</h3>
          <DataTable :value="tunnels" size="small" stripedRows>
            <Column field="id" header="ID" />
            <Column header="Remote"><template #body="{ data }">{{ data.server }} / {{ data.project }} / {{ data.service }}:{{ data.target_port }}</template></Column>
            <Column header="Local"><template #body="{ data }">{{ data.local_host }}:{{ data.local_port }}</template></Column>
            <Column header="Status"><template #body="{ data }"><Tag :value="data.status" :severity="statusSeverity(data.status)" /></template></Column>
            <Column header="">
              <template #body="{ data }">
                <Button
                  v-if="data.status === 'running'"
                  label="Stop"
                  icon="pi pi-stop"
                  size="small"
                  severity="danger"
                  outlined
                  @click="closeTunnel(data.id)"
                />
                <Button v-else label="Start" icon="pi pi-play" size="small" severity="success" outlined @click="startTunnel(data)" />
              </template>
            </Column>
          </DataTable>
        </div>
      </section>

      <section v-if="activeTab === 'Env'" class="page">
        <div class="toolbar">
          <Select v-model="selectedTunnel" :options="tunnelOptions" optionLabel="label" optionValue="value" placeholder="Select tunnel" />
          <Button label="Preview" icon="pi pi-eye" outlined @click="renderEnv" />
          <InputText v-model="envPath" />
          <Button label="Write" icon="pi pi-file-export" @click="writeEnv" />
        </div>
        <pre>{{ envPreview }}</pre>
      </section>

      <section v-if="activeTab === 'Logs'" class="page">
        <pre>{{ logs.join('\n') }}</pre>
      </section>

      <section v-if="activeTab === 'Settings'" class="page form settings">
        <label>Default local host<InputText v-model="defaults.local_host" /></label>
        <label>Default socat image<InputText v-model="defaults.socat_image" /></label>
        <label>socat command<InputText v-model="defaults.socat_command" /></label>
        <label>SSH binary<InputText v-model="defaults.ssh_binary" /></label>
        <label>Docker timeout seconds<InputNumber v-model="defaults.docker_timeout_secs" :min="1" :useGrouping="false" fluid /></label>
        <Button label="Save Settings" icon="pi pi-save" @click="saveDefaultSettings" />
      </section>
      </section>
    </ScrollPanel>
  </main>
</template>
