<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

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

const serverForm = reactive<ServerConfig>({
  name: "",
  host: "",
  port: 22,
  user: "",
  identity_file: "",
  ssh_alias: "",
  default_socat_image: "",
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
    await invoke("save_server", { server: compactServer(serverForm) });
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
    if (result.length > 0) {
      selectedProject.value = result[0].project;
      tunnelForm.server = selectedServer.value;
      tunnelForm.project = result[0].project;
      await loadServices(result[0].project);
    }
  }
}

async function loadServices(project: string) {
  selectedProject.value = project;
  tunnelForm.project = project;
  const result = await runTask(`Loaded services for ${project}`, async () =>
    invoke<ComposeService[]>("list_compose_services", {
      serverId: selectedServer.value,
      project,
    }),
  );
  if (result) {
    services.value = result;
    if (result.length > 0) {
      pickService(result[0]);
    }
  }
}

function pickService(service: ComposeService) {
  tunnelForm.service = service.service;
  tunnelForm.network = service.networks[0] ?? "";
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
    await refreshTunnels();
  });
}

async function closeTunnel(id: string) {
  await runTask(`Stopped tunnel ${id}`, async () => {
    await invoke("close_tunnel", { tunnelId: id });
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
  Object.assign(serverForm, {
    name: "",
    host: "",
    port: 22,
    user: "",
    identity_file: "",
    ssh_alias: "",
    default_socat_image: "",
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

function statusClass(status: string) {
  return status === "running" ? "tag tag-running" : status === "error" ? "tag tag-error" : "tag";
}

onMounted(bootstrap);
</script>

<template>
  <main class="shell">
    <aside class="sidebar">
      <div class="brand">
        <span class="brand-mark">CT</span>
        <div>
          <h1>Compose Tunnel</h1>
          <p>SSH to Compose internals</p>
        </div>
      </div>
      <nav>
        <button
          v-for="tab in tabs"
          :key="tab"
          :class="{ active: activeTab === tab }"
          type="button"
          @click="setTab(tab)"
        >
          {{ tab }}
        </button>
      </nav>
      <div class="config-path">{{ configDir }}</div>
    </aside>

    <section class="workspace">
      <header class="topbar">
        <div>
          <h2>{{ activeTab }}</h2>
          <p>{{ runningCount }} running tunnels, {{ stoppedCount }} stopped</p>
        </div>
        <button class="primary" type="button" :disabled="loading" @click="refreshTunnels">Refresh</button>
      </header>

      <div v-if="notice" class="notice">{{ notice }}</div>
      <div v-if="error" class="error">{{ error }}</div>

      <section v-if="activeTab === 'Dashboard'" class="page">
        <div class="metrics">
          <article>
            <span>Running</span>
            <strong>{{ runningCount }}</strong>
          </article>
          <article>
            <span>Servers</span>
            <strong>{{ servers.length }}</strong>
          </article>
          <article>
            <span>Projects</span>
            <strong>{{ projects.length }}</strong>
          </article>
        </div>
        <div class="toolbar">
          <button type="button" @click="setTab('Servers')">Add Server</button>
          <button type="button" @click="setTab('Compose')">Discover Compose</button>
          <button type="button" @click="setTab('Tunnels')">Open Tunnel</button>
        </div>
        <table>
          <thead>
            <tr><th>ID</th><th>Service</th><th>Local</th><th>Status</th></tr>
          </thead>
          <tbody>
            <tr v-for="tunnel in tunnels" :key="tunnel.id">
              <td>{{ tunnel.id }}</td>
              <td>{{ tunnel.project }}/{{ tunnel.service }}:{{ tunnel.target_port }}</td>
              <td>{{ tunnel.local_host }}:{{ tunnel.local_port }}</td>
              <td><span :class="statusClass(tunnel.status)">{{ tunnel.status }}</span></td>
            </tr>
          </tbody>
        </table>
      </section>

      <section v-if="activeTab === 'Servers'" class="page split">
        <form class="panel form" @submit.prevent="saveServer">
          <h3>Server</h3>
          <label>Name<input v-model="serverForm.name" required /></label>
          <label>Host<input v-model="serverForm.host" required /></label>
          <label>User<input v-model="serverForm.user" required /></label>
          <label>Port<input v-model.number="serverForm.port" type="number" min="1" /></label>
          <label>Identity file<input v-model="serverForm.identity_file" placeholder="~/.ssh/id_ed25519" /></label>
          <label>SSH config alias<input v-model="serverForm.ssh_alias" placeholder="staging" /></label>
          <label>Default socat image<input v-model="serverForm.default_socat_image" placeholder="alpine/socat:latest" /></label>
          <div class="toolbar"><button class="primary" type="submit">Save</button><button type="button" @click="clearServerForm">Clear</button></div>
        </form>
        <div class="panel">
          <h3>Servers</h3>
          <table>
            <thead><tr><th>Name</th><th>Host</th><th>User</th><th>Actions</th></tr></thead>
            <tbody>
              <tr v-for="server in servers" :key="server.name">
                <td>{{ server.name }}</td>
                <td>{{ server.host }}:{{ server.port }}</td>
                <td>{{ server.user }}</td>
                <td class="actions">
                  <button type="button" title="Test" @click="testServer(server.name)">Test</button>
                  <button type="button" title="Delete" @click="deleteServer(server.name)">Delete</button>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </section>

      <section v-if="activeTab === 'Compose'" class="page">
        <div class="toolbar">
          <select v-model="selectedServer">
            <option v-for="server in servers" :key="server.name" :value="server.name">{{ server.name }}</option>
          </select>
          <button class="primary" type="button" @click="discoverProjects">Discover</button>
        </div>
        <div class="project-grid">
          <article v-for="project in projects" :key="project.project" class="panel">
            <div class="row-between">
              <h3>{{ project.project }}</h3>
              <button type="button" @click="loadServices(project.project)">Services</button>
            </div>
            <p>{{ project.services.join(', ') }}</p>
          </article>
        </div>
        <table>
          <thead><tr><th>Service</th><th>Container</th><th>Status</th><th>Ports</th><th>Networks</th><th></th></tr></thead>
          <tbody>
            <tr v-for="service in services" :key="service.container">
              <td>{{ service.service }}</td>
              <td>{{ service.container }}</td>
              <td>{{ service.status }}</td>
              <td>{{ service.ports.join(', ') }}</td>
              <td>{{ service.networks.join(', ') }}</td>
              <td><button type="button" @click="pickService(service)">Tunnel</button></td>
            </tr>
          </tbody>
        </table>
      </section>

      <section v-if="activeTab === 'Tunnels'" class="page split">
        <form class="panel form" @submit.prevent="openTunnel">
          <h3>Create Tunnel</h3>
          <label>Server<input v-model="tunnelForm.server" list="servers-list" required /></label>
          <datalist id="servers-list"><option v-for="server in servers" :key="server.name" :value="server.name" /></datalist>
          <label>Project<input v-model="tunnelForm.project" required /></label>
          <label>Service<input v-model="tunnelForm.service" required /></label>
          <label>Network<input v-model="tunnelForm.network" placeholder="myapp_default" /></label>
          <label>Target port<input v-model.number="tunnelForm.target_port" type="number" min="1" required /></label>
          <label>Local port<input v-model="tunnelForm.local_port" placeholder="auto" /></label>
          <label>Env prefix<input v-model="tunnelForm.env_prefix" placeholder="DATABASE" /></label>
          <label>socat image<input v-model="tunnelForm.socat_image" :placeholder="defaults.socat_image" /></label>
          <button class="primary" type="submit">Start</button>
        </form>
        <div class="panel">
          <h3>Tunnels</h3>
          <table>
            <thead><tr><th>ID</th><th>Remote</th><th>Local</th><th>Status</th><th></th></tr></thead>
            <tbody>
              <tr v-for="tunnel in tunnels" :key="tunnel.id">
                <td>{{ tunnel.id }}</td>
                <td>{{ tunnel.server }} / {{ tunnel.project }} / {{ tunnel.service }}:{{ tunnel.target_port }}</td>
                <td>{{ tunnel.local_host }}:{{ tunnel.local_port }}</td>
                <td><span :class="statusClass(tunnel.status)">{{ tunnel.status }}</span></td>
                <td><button type="button" @click="closeTunnel(tunnel.id)">Stop</button></td>
              </tr>
            </tbody>
          </table>
        </div>
      </section>

      <section v-if="activeTab === 'Env'" class="page">
        <div class="toolbar">
          <select v-model="selectedTunnel">
            <option v-for="tunnel in tunnels" :key="tunnel.id" :value="tunnel.id">{{ tunnel.id }}</option>
          </select>
          <button type="button" @click="renderEnv">Preview</button>
          <input v-model="envPath" />
          <button class="primary" type="button" @click="writeEnv">Write</button>
        </div>
        <pre>{{ envPreview }}</pre>
      </section>

      <section v-if="activeTab === 'Logs'" class="page">
        <pre>{{ logs.join('\n') }}</pre>
      </section>

      <section v-if="activeTab === 'Settings'" class="page form settings">
        <label>Default local host<input v-model="defaults.local_host" /></label>
        <label>Default socat image<input v-model="defaults.socat_image" /></label>
        <label>socat command<input v-model="defaults.socat_command" /></label>
        <label>SSH binary<input v-model="defaults.ssh_binary" /></label>
        <label>Docker timeout seconds<input v-model.number="defaults.docker_timeout_secs" type="number" min="1" /></label>
        <button class="primary" type="button" @click="saveDefaultSettings">Save Settings</button>
      </section>
    </section>
  </main>
</template>
