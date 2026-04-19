<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

// ── Types ────────────────────────────────────────────────────────────────────

interface VpnStatus {
  connected: boolean
  location: string | null
  ip: string | null
}

interface Location {
  iso: string
  country: string
  city: string
  ping: number | null
}

interface LicenseInfo {
  logged_in: boolean
  email: string | null
  plan: string | null
  devices: string | null
  valid_until: string | null
}

interface ConfigInfo {
  mode: string
  dns: string
  protocol: string
  tunnel_routing_mode: string
  update_channel: string
  change_system_dns: boolean
  post_quantum: boolean
  crash_reporting: boolean
  telemetry: boolean
  debug_logging: boolean
  show_notifications: boolean
}

// ── Tab state ─────────────────────────────────────────────────────────────────

type Tab = 'home' | 'locations' | 'exclusions' | 'account'
const currentTab = ref<Tab>('home')

// ── Data ──────────────────────────────────────────────────────────────────────

const status = ref<VpnStatus>({ connected: false, location: null, ip: null })
const locations = ref<Location[]>([])
const license = ref<LicenseInfo | null>(null)
const config = ref<ConfigInfo | null>(null)

const locationSearch = ref('')
const selectedLocation = ref<Location | null>(null)
const useFastest = ref(false)

const exclusionsMode = ref('')
const exclusionsList = ref<string[]>([])
const newExclusion = ref('')

const connecting = ref(false)
const loadingLocations = ref(false)
const loadingAccount = ref(false)
const loadingConfig = ref(false)

const statusError = ref('')
const connectError = ref('')
const locationsError = ref('')
const exclusionsError = ref('')
const configError = ref('')

const configDns = ref('')
const configMode = ref('')
const configProtocol = ref('')

let statusInterval: ReturnType<typeof setInterval> | null = null

// ── Sudo modal ────────────────────────────────────────────────────────────────

const showSudoModal = ref(false)
const sudoInputPassword = ref('')
const sudoModalError = ref('')
const sudoModalLoading = ref(false)

// Non-reactive: callbacks for the in-flight privileged operation.
let _sudoResolve: ((pwd: string | null) => void) | null = null
let _currentSudoFn: ((pwd?: string) => Promise<unknown>) | null = null

/**
 * Wraps a privileged Tauri command:
 * 1. Tries without a password (uses sudo -n -E, succeeds if cache is warm).
 * 2. On SUDO_PASSWORD_REQUIRED opens the modal and waits for the user.
 * 3. On SUDO_AUTH_FAILED (wrong password) keeps the modal open with an error
 *    so the user can retry without triggering the action again.
 */
async function runWithSudo<T>(fn: (pwd?: string) => Promise<T>): Promise<T> {
  // First attempt — no password, uses sudo credential cache.
  try {
    return await fn(undefined)
  } catch (e) {
    if (!String(e).includes('SUDO_PASSWORD_REQUIRED')) throw e
  }

  // Cache miss — show modal and hand off to the promise below.
  return new Promise<T>((resolve, reject) => {
    sudoInputPassword.value = ''
    sudoModalError.value = ''
    sudoModalLoading.value = false
    showSudoModal.value = true

    _currentSudoFn = fn as (pwd?: string) => Promise<unknown>
    _sudoResolve = (pwd: string | null) => {
      if (pwd === null) {
        reject(new Error('Cancelled'))
        return
      }
      sudoModalLoading.value = true
      sudoModalError.value = ''
      ;(fn(pwd) as Promise<T>)
        .then(result => {
          showSudoModal.value = false
          _currentSudoFn = null
          _sudoResolve = null
          resolve(result)
        })
        .catch(err => {
          sudoModalLoading.value = false
          const msg = String(err)
          if (msg.includes('SUDO_AUTH_FAILED')) {
            // Keep modal open — let user try again.
            sudoModalError.value = 'Incorrect password. Try again.'
            sudoInputPassword.value = ''
          } else {
            showSudoModal.value = false
            _currentSudoFn = null
            _sudoResolve = null
            reject(err)
          }
        })
    }
  })
}

function onSudoConfirm() {
  if (_sudoResolve) _sudoResolve(sudoInputPassword.value)
}

function onSudoCancel() {
  showSudoModal.value = false
  if (_sudoResolve) _sudoResolve(null)
  _sudoResolve = null
  _currentSudoFn = null
}

// ── Computed ──────────────────────────────────────────────────────────────────

const filteredLocations = computed(() => {
  const q = locationSearch.value.toLowerCase().trim()
  if (!q) return locations.value
  return locations.value.filter(
    l =>
      l.city.toLowerCase().includes(q) ||
      l.country.toLowerCase().includes(q) ||
      l.iso.toLowerCase().includes(q),
  )
})

const displayLocation = computed(() => {
  if (status.value.connected && status.value.location) return status.value.location
  if (selectedLocation.value)
    return `${selectedLocation.value.city}, ${selectedLocation.value.country}`
  if (useFastest.value) return 'Fastest location'
  return 'No location selected'
})

// ── Helpers ───────────────────────────────────────────────────────────────────

function countryFlag(iso: string): string {
  return iso
    .toUpperCase()
    .split('')
    .map(c => String.fromCodePoint(0x1f1e0 + c.charCodeAt(0) - 65))
    .join('')
}

function pingClass(ping: number | null): string {
  if (ping === null) return 'ping-unknown'
  if (ping <= 50) return 'ping-good'
  if (ping <= 120) return 'ping-ok'
  return 'ping-bad'
}

// ── Data loading ──────────────────────────────────────────────────────────────

async function refreshStatus() {
  try {
    status.value = await invoke<VpnStatus>('get_status')
    statusError.value = ''
  } catch (e) {
    statusError.value = String(e)
  }
}

async function loadLocations() {
  loadingLocations.value = true
  locationsError.value = ''
  try {
    locations.value = await invoke<Location[]>('list_locations')
  } catch (e) {
    locationsError.value = String(e)
  } finally {
    loadingLocations.value = false
  }
}

async function loadAccount() {
  loadingAccount.value = true
  try {
    license.value = await invoke<LicenseInfo>('get_license')
  } catch {
    license.value = null
  } finally {
    loadingAccount.value = false
  }
}

async function loadConfig() {
  loadingConfig.value = true
  configError.value = ''
  try {
    const cfg = await invoke<ConfigInfo>('get_config')
    config.value = cfg
    configDns.value = cfg.dns
    configMode.value = cfg.mode
    configProtocol.value = cfg.protocol
  } catch (e) {
    configError.value = String(e)
  } finally {
    loadingConfig.value = false
  }
}

async function loadExclusions() {
  exclusionsError.value = ''
  try {
    const [mode, list] = await invoke<[string, string[]]>('get_exclusions')
    exclusionsMode.value = mode
    exclusionsList.value = list
  } catch (e) {
    exclusionsError.value = String(e)
  }
}

// ── Actions ───────────────────────────────────────────────────────────────────

async function connect() {
  connecting.value = true
  connectError.value = ''
  try {
    await runWithSudo(pwd =>
      invoke('vpn_connect', {
        location: useFastest.value ? null : (selectedLocation.value?.city ?? null),
        fastest: useFastest.value,
        sudoPassword: pwd ?? null,
      }),
    )
    for (let i = 0; i < 30; i++) {
      await new Promise(r => setTimeout(r, 1000))
      await refreshStatus()
      if (status.value.connected) break
    }
  } catch (e) {
    const msg = String(e)
    if (!msg.includes('Cancelled')) connectError.value = msg
  } finally {
    connecting.value = false
  }
}

async function disconnect() {
  connecting.value = true
  connectError.value = ''
  try {
    await runWithSudo(pwd => invoke('vpn_disconnect', { sudoPassword: pwd ?? null }))
    await new Promise(r => setTimeout(r, 500))
    await refreshStatus()
  } catch (e) {
    const msg = String(e)
    if (!msg.includes('Cancelled')) connectError.value = msg
  } finally {
    connecting.value = false
  }
}

function selectLocation(loc: Location) {
  selectedLocation.value = loc
  useFastest.value = false
  currentTab.value = 'home'
}

async function handleLogout() {
  try {
    await runWithSudo(pwd => invoke('vpn_logout', { sudoPassword: pwd ?? null }))
    license.value = null
    await loadAccount()
  } catch { /* cancelled or error */ }
}

async function saveConfigMode() {
  configError.value = ''
  try {
    await runWithSudo(pwd =>
      invoke('config_set_mode', { mode: configMode.value, sudoPassword: pwd ?? null }),
    )
    await loadConfig()
  } catch (e) {
    const msg = String(e)
    if (!msg.includes('Cancelled')) configError.value = msg
  }
}

async function saveConfigDns() {
  configError.value = ''
  try {
    await runWithSudo(pwd =>
      invoke('config_set_dns', { dns: configDns.value, sudoPassword: pwd ?? null }),
    )
    await loadConfig()
  } catch (e) {
    const msg = String(e)
    if (!msg.includes('Cancelled')) configError.value = msg
  }
}

async function saveConfigProtocol() {
  configError.value = ''
  try {
    await runWithSudo(pwd =>
      invoke('config_set_protocol', { protocol: configProtocol.value, sudoPassword: pwd ?? null }),
    )
    await loadConfig()
  } catch (e) {
    const msg = String(e)
    if (!msg.includes('Cancelled')) configError.value = msg
  }
}

async function togglePostQuantum() {
  if (!config.value) return
  const newVal = !config.value.post_quantum
  try {
    await runWithSudo(pwd =>
      invoke('config_set_post_quantum', { enabled: newVal, sudoPassword: pwd ?? null }),
    )
    config.value.post_quantum = newVal
  } catch (e) {
    const msg = String(e)
    if (!msg.includes('Cancelled')) configError.value = msg
  }
}

async function toggleCrashReporting() {
  if (!config.value) return
  const newVal = !config.value.crash_reporting
  try {
    await runWithSudo(pwd =>
      invoke('config_set_crash_reporting', { enabled: newVal, sudoPassword: pwd ?? null }),
    )
    config.value.crash_reporting = newVal
  } catch (e) {
    const msg = String(e)
    if (!msg.includes('Cancelled')) configError.value = msg
  }
}

async function toggleTelemetry() {
  if (!config.value) return
  const newVal = !config.value.telemetry
  try {
    await runWithSudo(pwd =>
      invoke('config_set_telemetry', { enabled: newVal, sudoPassword: pwd ?? null }),
    )
    config.value.telemetry = newVal
  } catch (e) {
    const msg = String(e)
    if (!msg.includes('Cancelled')) configError.value = msg
  }
}

async function handleAddExclusion() {
  const site = newExclusion.value.trim()
  if (!site) return
  try {
    await runWithSudo(pwd =>
      invoke('add_exclusion', { site, sudoPassword: pwd ?? null }),
    )
    newExclusion.value = ''
    await loadExclusions()
  } catch (e) {
    const msg = String(e)
    if (!msg.includes('Cancelled')) exclusionsError.value = msg
  }
}

async function handleRemoveExclusion(site: string) {
  try {
    await runWithSudo(pwd =>
      invoke('remove_exclusion', { site, sudoPassword: pwd ?? null }),
    )
    await loadExclusions()
  } catch (e) {
    const msg = String(e)
    if (!msg.includes('Cancelled')) exclusionsError.value = msg
  }
}

async function switchExclusionsMode(mode: string) {
  try {
    await runWithSudo(pwd =>
      invoke('set_exclusions_mode', { mode, sudoPassword: pwd ?? null }),
    )
    await loadExclusions()
  } catch (e) {
    const msg = String(e)
    if (!msg.includes('Cancelled')) exclusionsError.value = msg
  }
}

// ── Tab switching ─────────────────────────────────────────────────────────────

function switchTab(tab: Tab) {
  currentTab.value = tab
  if (tab === 'locations' && locations.value.length === 0) loadLocations()
  if (tab === 'account' && license.value === null) loadAccount()
  if (tab === 'account' && config.value === null) loadConfig()
  if (tab === 'exclusions') loadExclusions()
}

// ── Lifecycle ──────────────────────────────────────────────────────────────────

onMounted(async () => {
  await refreshStatus()
  statusInterval = setInterval(refreshStatus, 5000)
})

onUnmounted(() => {
  if (statusInterval) clearInterval(statusInterval)
})
</script>

<template>
  <div class="app">

    <!-- ── Sudo password modal ── -->
    <Transition name="modal">
      <div v-if="showSudoModal" class="modal-overlay" @click.self="onSudoCancel">
        <div class="modal-card">
          <div class="modal-icon">
            <svg width="28" height="28" viewBox="0 0 24 24" fill="none">
              <rect x="3" y="11" width="18" height="11" rx="2" stroke="#74C045" stroke-width="1.8"/>
              <path d="M7 11V7a5 5 0 0110 0v4" stroke="#74C045" stroke-width="1.8" stroke-linecap="round"/>
            </svg>
          </div>
          <h3 class="modal-title">Authentication required</h3>
          <p class="modal-subtitle">Enter your user password to continue.</p>

          <input
            v-model="sudoInputPassword"
            type="password"
            class="modal-input"
            placeholder="Password"
            autofocus
            :disabled="sudoModalLoading"
            @keyup.enter="onSudoConfirm"
          />

          <p v-if="sudoModalError" class="modal-error">{{ sudoModalError }}</p>

          <div class="modal-actions">
            <button class="modal-cancel" :disabled="sudoModalLoading" @click="onSudoCancel">
              Cancel
            </button>
            <button
              class="modal-confirm"
              :disabled="sudoModalLoading || !sudoInputPassword"
              @click="onSudoConfirm"
            >
              <span v-if="sudoModalLoading" class="btn-spinner"></span>
              <span v-else>Authenticate</span>
            </button>
          </div>
        </div>
      </div>
    </Transition>

    <!-- ── Header ── -->
    <header class="header">
      <div class="header-logo">
        <svg width="22" height="22" viewBox="0 0 24 24" fill="none">
          <path d="M12 2L3 7v5c0 5.25 3.84 10.15 9 11.33C17.16 22.15 21 17.25 21 12V7L12 2z"
            fill="#74C045" />
        </svg>
        <span class="header-title">AdGuard VPN</span>
      </div>
      <div class="header-status" :class="status.connected ? 'status-on' : 'status-off'">
        {{ status.connected ? 'ON' : 'OFF' }}
      </div>
    </header>

    <!-- ── Main content ── -->
    <main class="content">

      <!-- HOME TAB -->
      <div v-if="currentTab === 'home'" class="tab-pane">
        <div class="orb-section">
          <div class="orb" :class="{ 'orb-connected': status.connected, 'orb-connecting': connecting }">
            <div class="orb-inner">
              <svg v-if="status.connected" width="36" height="36" viewBox="0 0 24 24" fill="none">
                <path d="M12 2L3 7v5c0 5.25 3.84 10.15 9 11.33C17.16 22.15 21 17.25 21 12V7L12 2z" fill="white" />
              </svg>
              <svg v-else-if="connecting" width="36" height="36" viewBox="0 0 24 24" fill="none" class="spin">
                <circle cx="12" cy="12" r="9" stroke="white" stroke-width="2.5" stroke-dasharray="30 20" />
              </svg>
              <svg v-else width="36" height="36" viewBox="0 0 24 24" fill="none">
                <path d="M12 2L3 7v5c0 5.25 3.84 10.15 9 11.33C17.16 22.15 21 17.25 21 12V7L12 2z"
                  stroke="white" stroke-width="2" fill="none" />
              </svg>
            </div>
          </div>
          <div class="orb-label">
            <span v-if="connecting">{{ status.connected ? 'Disconnecting...' : 'Connecting...' }}</span>
            <span v-else-if="status.connected">Connected</span>
            <span v-else>Disconnected</span>
          </div>
          <div v-if="status.ip" class="orb-ip">{{ status.ip }}</div>
        </div>

        <div class="location-card" @click="!status.connected && switchTab('locations')">
          <div class="location-flag">{{ selectedLocation ? countryFlag(selectedLocation.iso) : '🌐' }}</div>
          <div class="location-info">
            <span class="location-label">{{ status.connected ? 'Connected to' : 'Connect to' }}</span>
            <span class="location-name">{{ displayLocation }}</span>
          </div>
          <svg v-if="!status.connected" width="18" height="18" viewBox="0 0 24 24" fill="none" class="location-chevron">
            <path d="M9 18l6-6-6-6" stroke="#8B8FA8" stroke-width="2" stroke-linecap="round" />
          </svg>
        </div>

        <div v-if="!status.connected" class="fastest-row">
          <label class="toggle-label">
            <input type="checkbox" v-model="useFastest" @change="useFastest && (selectedLocation = null)" />
            <span class="toggle-track"><span class="toggle-thumb"></span></span>
            <span>Use fastest location</span>
          </label>
        </div>

        <button
          class="connect-btn"
          :class="{ 'btn-disconnect': status.connected, 'btn-loading': connecting }"
          :disabled="connecting"
          @click="status.connected ? disconnect() : connect()"
        >
          <span v-if="connecting">{{ status.connected ? 'Disconnecting...' : 'Connecting...' }}</span>
          <span v-else-if="status.connected">Disconnect</span>
          <span v-else>Connect</span>
        </button>

        <div v-if="connectError" class="error-box">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" style="flex-shrink:0">
            <circle cx="12" cy="12" r="10" stroke="#FF6B6B" stroke-width="2"/>
            <path d="M12 8v4M12 16h.01" stroke="#FF6B6B" stroke-width="2" stroke-linecap="round"/>
          </svg>
          <pre>{{ connectError }}</pre>
        </div>
        <div v-if="statusError" class="error-box">{{ statusError }}</div>
      </div>

      <!-- LOCATIONS TAB -->
      <div v-if="currentTab === 'locations'" class="tab-pane tab-locations">
        <div class="search-row">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" class="search-icon">
            <circle cx="11" cy="11" r="7" stroke="#8B8FA8" stroke-width="2"/>
            <path d="M20 20l-3.5-3.5" stroke="#8B8FA8" stroke-width="2" stroke-linecap="round"/>
          </svg>
          <input v-model="locationSearch" class="search-input" placeholder="Search locations..." autofocus />
          <button v-if="locationSearch" class="search-clear" @click="locationSearch = ''">×</button>
        </div>

        <div v-if="loadingLocations" class="loading-state">
          <div class="spinner"></div><span>Loading locations...</span>
        </div>
        <div v-else-if="locationsError" class="error-box" style="margin:16px">{{ locationsError }}</div>
        <div v-else class="locations-list">
          <div class="location-item" :class="{ 'location-item-selected': useFastest }"
            @click="useFastest = true; selectedLocation = null; currentTab = 'home'">
            <span class="loc-flag">⚡</span>
            <div class="loc-text">
              <span class="loc-city">Fastest location</span>
              <span class="loc-country">Automatically selected</span>
            </div>
          </div>
          <div
            v-for="loc in filteredLocations"
            :key="`${loc.iso}-${loc.city}`"
            class="location-item"
            :class="{ 'location-item-selected': !useFastest && selectedLocation?.city === loc.city && selectedLocation?.iso === loc.iso }"
            @click="selectLocation(loc)"
          >
            <span class="loc-flag">{{ countryFlag(loc.iso) }}</span>
            <div class="loc-text">
              <span class="loc-city">{{ loc.city }}</span>
              <span class="loc-country">{{ loc.country }}</span>
            </div>
            <span v-if="loc.ping !== null" class="loc-ping" :class="pingClass(loc.ping)">{{ loc.ping }}ms</span>
          </div>
          <div v-if="filteredLocations.length === 0" class="empty-state">
            No locations match "{{ locationSearch }}"
          </div>
        </div>
      </div>

      <!-- EXCLUSIONS TAB -->
      <div v-if="currentTab === 'exclusions'" class="tab-pane">
        <div class="section-title">Site Exclusions</div>
        <div class="mode-toggle-row">
          <button class="mode-btn" :class="{ active: exclusionsMode.toLowerCase().includes('general') }"
            @click="switchExclusionsMode('general')">General</button>
          <button class="mode-btn" :class="{ active: exclusionsMode.toLowerCase().includes('selective') }"
            @click="switchExclusionsMode('selective')">Selective</button>
        </div>
        <div class="mode-hint">
          <span v-if="exclusionsMode.toLowerCase().includes('general')">VPN is active for all sites <em>except</em> listed exclusions.</span>
          <span v-else-if="exclusionsMode.toLowerCase().includes('selective')">VPN is active <em>only</em> for listed sites.</span>
        </div>
        <div class="add-exclusion-row">
          <input v-model="newExclusion" class="exclusion-input" placeholder="example.com" @keyup.enter="handleAddExclusion" />
          <button class="add-btn" @click="handleAddExclusion">Add</button>
        </div>
        <div v-if="exclusionsError" class="error-box">{{ exclusionsError }}</div>
        <div class="exclusions-list">
          <div v-if="exclusionsList.length === 0" class="empty-state">No exclusions configured.</div>
          <div v-for="site in exclusionsList" :key="site" class="exclusion-item">
            <span class="exclusion-name">{{ site }}</span>
            <button class="remove-btn" @click="handleRemoveExclusion(site)">×</button>
          </div>
        </div>
      </div>

      <!-- ACCOUNT TAB -->
      <div v-if="currentTab === 'account'" class="tab-pane">
        <div v-if="loadingAccount" class="loading-state"><div class="spinner"></div></div>
        <template v-else-if="license && license.logged_in">
          <div class="account-avatar">{{ license.email?.charAt(0).toUpperCase() ?? '?' }}</div>
          <div class="account-email">{{ license.email }}</div>
          <div class="account-badge" :class="license.plan === 'PREMIUM' ? 'badge-premium' : 'badge-free'">
            {{ license.plan ?? 'FREE' }}
          </div>
          <div class="account-cards">
            <div class="account-card">
              <span class="card-label">Devices</span>
              <span class="card-value">{{ license.devices ?? '—' }}</span>
            </div>
            <div class="account-card">
              <span class="card-label">Valid until</span>
              <span class="card-value">{{ license.valid_until ?? '—' }}</span>
            </div>
          </div>

          <div class="section-title" style="margin-top:24px">Settings</div>
          <div v-if="loadingConfig" class="loading-state"><div class="spinner"></div></div>
          <div v-else-if="!config"><button class="secondary-btn" @click="loadConfig">Load settings</button></div>
          <template v-else>
            <div class="settings-group">
              <div class="setting-row">
                <div class="setting-info">
                  <span class="setting-name">VPN Mode</span>
                  <span class="setting-desc">TUN or SOCKS proxy</span>
                </div>
                <select v-model="configMode" class="setting-select" @change="saveConfigMode">
                  <option value="tun">TUN</option>
                  <option value="socks">SOCKS</option>
                </select>
              </div>
              <div class="setting-row">
                <div class="setting-info"><span class="setting-name">Protocol</span></div>
                <select v-model="configProtocol" class="setting-select" @change="saveConfigProtocol">
                  <option value="auto">Auto</option>
                  <option value="http2">HTTP/2</option>
                  <option value="quic">QUIC</option>
                </select>
              </div>
              <div class="setting-row">
                <div class="setting-info">
                  <span class="setting-name">Post-Quantum</span>
                  <span class="setting-desc">Quantum-resistant encryption</span>
                </div>
                <button class="toggle-btn" :class="{ 'toggle-on': config.post_quantum }" @click="togglePostQuantum">
                  <span class="toggle-knob"></span>
                </button>
              </div>
              <div class="setting-row">
                <div class="setting-info"><span class="setting-name">Crash Reporting</span></div>
                <button class="toggle-btn" :class="{ 'toggle-on': config.crash_reporting }" @click="toggleCrashReporting">
                  <span class="toggle-knob"></span>
                </button>
              </div>
              <div class="setting-row">
                <div class="setting-info">
                  <span class="setting-name">Analytics</span>
                  <span class="setting-desc">Anonymized usage data</span>
                </div>
                <button class="toggle-btn" :class="{ 'toggle-on': config.telemetry }" @click="toggleTelemetry">
                  <span class="toggle-knob"></span>
                </button>
              </div>
            </div>
            <div v-if="configError" class="error-box" style="margin-top:10px">{{ configError }}</div>
          </template>

          <button class="logout-btn" @click="handleLogout">Log out</button>
        </template>
        <div v-else class="not-logged-in">
          <svg width="48" height="48" viewBox="0 0 24 24" fill="none">
            <circle cx="12" cy="8" r="4" stroke="#8B8FA8" stroke-width="1.5"/>
            <path d="M4 20c0-4 3.58-7 8-7s8 3 8 7" stroke="#8B8FA8" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
          <p>Not logged in</p>
          <p class="hint-text">Run <code>adguardvpn-cli login</code> in a terminal to log in.</p>
        </div>
      </div>

    </main>

    <!-- ── Bottom nav ── -->
    <nav class="bottom-nav">
      <button class="nav-btn" :class="{ active: currentTab === 'home' }" @click="switchTab('home')">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none">
          <path d="M12 2L3 7v5c0 5.25 3.84 10.15 9 11.33C17.16 22.15 21 17.25 21 12V7L12 2z"
            stroke="currentColor" stroke-width="1.8" fill="none" />
        </svg>
        <span>Home</span>
      </button>
      <button class="nav-btn" :class="{ active: currentTab === 'locations' }" @click="switchTab('locations')">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none">
          <circle cx="12" cy="12" r="9" stroke="currentColor" stroke-width="1.8"/>
          <path d="M2 12h20M12 2a15.3 15.3 0 010 20M12 2a15.3 15.3 0 000 20" stroke="currentColor" stroke-width="1.8"/>
        </svg>
        <span>Locations</span>
      </button>
      <button class="nav-btn" :class="{ active: currentTab === 'exclusions' }" @click="switchTab('exclusions')">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none">
          <circle cx="12" cy="12" r="9" stroke="currentColor" stroke-width="1.8"/>
          <path d="M4.93 4.93l14.14 14.14" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>
        </svg>
        <span>Exclusions</span>
      </button>
      <button class="nav-btn" :class="{ active: currentTab === 'account' }" @click="switchTab('account')">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none">
          <circle cx="12" cy="8" r="4" stroke="currentColor" stroke-width="1.8"/>
          <path d="M4 20c0-4 3.58-7 8-7s8 3 8 7" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>
        </svg>
        <span>Account</span>
      </button>
    </nav>
  </div>
</template>

<style>
*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

:root {
  --bg: #0F1117; --surface: #1A1D27; --surface2: #22263A; --border: #2D3044;
  --green: #74C045; --green-dim: #3d6424; --red: #FF6B6B;
  --text: #F2F4F8; --muted: #8B8FA8; --radius: 12px;
  font-family: 'Inter', system-ui, sans-serif; font-size: 14px;
  color: var(--text); background: var(--bg);
}
body { background: var(--bg); overflow: hidden; }
.app { display: flex; flex-direction: column; height: 100vh; background: var(--bg); overflow: hidden; }

/* ── Modal ── */
.modal-overlay {
  position: fixed; inset: 0; background: rgba(0,0,0,0.65);
  backdrop-filter: blur(4px); display: flex; align-items: center;
  justify-content: center; z-index: 100;
}
.modal-card {
  background: var(--surface); border: 1px solid var(--border); border-radius: 16px;
  padding: 28px 24px 22px; width: 300px;
  display: flex; flex-direction: column; align-items: center; gap: 10px;
}
.modal-icon {
  width: 52px; height: 52px; background: rgba(116,192,69,0.1);
  border-radius: 50%; display: flex; align-items: center; justify-content: center; margin-bottom: 4px;
}
.modal-title { font-size: 16px; font-weight: 700; }
.modal-subtitle { font-size: 12px; color: var(--muted); text-align: center; margin-bottom: 4px; }
.modal-input {
  width: 100%; background: var(--surface2); border: 1px solid var(--border);
  border-radius: 8px; padding: 10px 12px; color: var(--text); font-size: 14px;
  outline: none; transition: border-color 0.2s;
}
.modal-input:focus { border-color: var(--green); }
.modal-input:disabled { opacity: 0.5; }
.modal-error { font-size: 12px; color: var(--red); width: 100%; text-align: center; }
.modal-actions { display: flex; gap: 8px; width: 100%; margin-top: 4px; }
.modal-cancel {
  flex: 1; padding: 10px; border-radius: 8px; border: 1px solid var(--border);
  background: var(--surface2); color: var(--muted); cursor: pointer; font-size: 13px;
}
.modal-cancel:disabled { opacity: 0.5; cursor: not-allowed; }
.modal-confirm {
  flex: 2; padding: 10px; border-radius: 8px; border: none;
  background: var(--green); color: #0a1a04; font-size: 13px; font-weight: 700;
  cursor: pointer; display: flex; align-items: center; justify-content: center; gap: 6px;
}
.modal-confirm:disabled { opacity: 0.5; cursor: not-allowed; }
.btn-spinner {
  width: 14px; height: 14px; border: 2px solid rgba(10,26,4,0.3);
  border-top-color: #0a1a04; border-radius: 50%; animation: spin 0.7s linear infinite;
}
.modal-enter-active, .modal-leave-active { transition: opacity 0.15s ease; }
.modal-enter-active .modal-card, .modal-leave-active .modal-card { transition: transform 0.15s ease; }
.modal-enter-from, .modal-leave-to { opacity: 0; }
.modal-enter-from .modal-card, .modal-leave-to .modal-card { transform: scale(0.95); }

/* ── Header ── */
.header {
  display: flex; align-items: center; justify-content: space-between;
  padding: 14px 18px; background: var(--surface); border-bottom: 1px solid var(--border); flex-shrink: 0;
}
.header-logo { display: flex; align-items: center; gap: 8px; }
.header-title { font-weight: 600; font-size: 15px; }
.header-status { font-size: 11px; font-weight: 700; letter-spacing: 0.08em; padding: 3px 8px; border-radius: 20px; }
.status-on  { background: rgba(116,192,69,0.18); color: var(--green); }
.status-off { background: rgba(139,143,168,0.15); color: var(--muted); }

/* ── Content ── */
.content { flex: 1; overflow-y: auto; overflow-x: hidden; scrollbar-width: thin; scrollbar-color: var(--border) transparent; }
.tab-pane { padding: 20px 18px; display: flex; flex-direction: column; align-items: center; min-height: 100%; }

/* ── Orb ── */
.orb-section { display: flex; flex-direction: column; align-items: center; margin: 20px 0 24px; }
.orb {
  width: 110px; height: 110px; border-radius: 50%; background: var(--surface2);
  display: flex; align-items: center; justify-content: center;
  border: 2px solid var(--border); transition: all 0.4s ease;
}
.orb-connected { background: radial-gradient(circle,#3a6b1f 0%,#1e3a0f 100%); border-color: var(--green); box-shadow: 0 0 32px rgba(116,192,69,0.35); }
.orb-connecting { border-color: rgba(116,192,69,0.5); animation: orb-pulse 1.5s ease-in-out infinite; }
@keyframes orb-pulse { 0%,100%{box-shadow:0 0 0 0 rgba(116,192,69,0.3)} 50%{box-shadow:0 0 0 14px rgba(116,192,69,0)} }
.orb-inner { display: flex; align-items: center; justify-content: center; }
.orb-label { margin-top: 12px; font-size: 17px; font-weight: 600; }
.orb-ip { margin-top: 4px; font-size: 12px; color: var(--muted); font-family: monospace; }
.spin { animation: spin 1s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }

/* ── Location card ── */
.location-card {
  width: 100%; background: var(--surface); border: 1px solid var(--border);
  border-radius: var(--radius); padding: 14px 16px;
  display: flex; align-items: center; gap: 12px; cursor: pointer;
  transition: border-color 0.2s; margin-bottom: 12px;
}
.location-card:hover { border-color: rgba(116,192,69,0.4); }
.location-flag { font-size: 24px; line-height: 1; }
.location-info { flex: 1; display: flex; flex-direction: column; gap: 2px; }
.location-label { font-size: 11px; color: var(--muted); text-transform: uppercase; letter-spacing: 0.06em; }
.location-name { font-size: 14px; font-weight: 500; }
.location-chevron { flex-shrink: 0; }

/* ── Fastest toggle ── */
.fastest-row { width: 100%; margin-bottom: 16px; }
.toggle-label { display: flex; align-items: center; gap: 10px; cursor: pointer; font-size: 13px; color: var(--muted); }
.toggle-label input { display: none; }
.toggle-track { width: 36px; height: 20px; background: var(--surface2); border-radius: 10px; position: relative; border: 1px solid var(--border); flex-shrink: 0; transition: background 0.2s; }
.toggle-label input:checked + .toggle-track { background: var(--green-dim); border-color: var(--green); }
.toggle-thumb { position: absolute; top: 2px; left: 2px; width: 14px; height: 14px; background: var(--muted); border-radius: 50%; transition: transform 0.2s, background 0.2s; }
.toggle-label input:checked + .toggle-track .toggle-thumb { transform: translateX(16px); background: var(--green); }

/* ── Connect button ── */
.connect-btn {
  width: 100%; padding: 14px; border-radius: var(--radius); border: none;
  background: var(--green); color: #0a1a04; font-size: 15px; font-weight: 700;
  cursor: pointer; transition: opacity 0.2s, transform 0.1s; letter-spacing: 0.02em;
}
.connect-btn:hover:not(:disabled) { opacity: 0.9; }
.connect-btn:active:not(:disabled) { transform: scale(0.98); }
.connect-btn:disabled { opacity: 0.5; cursor: not-allowed; }
.btn-disconnect { background: var(--surface2); color: var(--text); border: 1px solid var(--border); }
.btn-loading { background: var(--surface2); color: var(--muted); }

/* ── Error ── */
.error-box {
  width: 100%; margin-top: 12px; background: rgba(255,107,107,0.08);
  border: 1px solid rgba(255,107,107,0.25); border-radius: 8px;
  padding: 10px 14px; color: #FF6B6B; font-size: 12px; display: flex; gap: 8px; align-items: flex-start;
}
.error-box pre { white-space: pre-wrap; word-break: break-word; font-family: monospace; }

/* ── Locations ── */
.tab-locations { padding: 0; align-items: stretch; }
.search-row { display: flex; align-items: center; gap: 8px; padding: 12px 16px; background: var(--surface); border-bottom: 1px solid var(--border); position: sticky; top: 0; z-index: 1; }
.search-input { flex: 1; background: transparent; border: none; outline: none; color: var(--text); font-size: 14px; }
.search-input::placeholder { color: var(--muted); }
.search-clear { background: none; border: none; color: var(--muted); font-size: 18px; cursor: pointer; padding: 0 4px; line-height: 1; }
.locations-list { width: 100%; }
.location-item { display: flex; align-items: center; gap: 12px; padding: 12px 16px; cursor: pointer; border-bottom: 1px solid var(--border); transition: background 0.15s; }
.location-item:hover { background: var(--surface2); }
.location-item-selected { background: rgba(116,192,69,0.08); }
.location-item-selected .loc-city { color: var(--green); }
.loc-flag { font-size: 22px; width: 30px; text-align: center; flex-shrink: 0; }
.loc-text { flex: 1; display: flex; flex-direction: column; gap: 2px; }
.loc-city { font-size: 14px; font-weight: 500; }
.loc-country { font-size: 12px; color: var(--muted); }
.loc-ping { font-size: 11px; font-weight: 600; padding: 3px 7px; border-radius: 20px; font-family: monospace; }
.ping-good { background: rgba(116,192,69,0.15); color: var(--green); }
.ping-ok   { background: rgba(255,193,7,0.15); color: #ffc107; }
.ping-bad  { background: rgba(255,107,107,0.15); color: var(--red); }

/* ── Exclusions ── */
.section-title { width: 100%; font-size: 13px; font-weight: 600; color: var(--muted); text-transform: uppercase; letter-spacing: 0.08em; margin-bottom: 12px; }
.mode-toggle-row { display: flex; gap: 8px; width: 100%; margin-bottom: 8px; }
.mode-btn { flex: 1; padding: 9px; border-radius: 8px; border: 1px solid var(--border); background: var(--surface); color: var(--muted); cursor: pointer; font-size: 13px; font-weight: 500; transition: all 0.2s; }
.mode-btn.active { background: rgba(116,192,69,0.12); border-color: var(--green); color: var(--green); }
.mode-hint { width: 100%; font-size: 12px; color: var(--muted); margin-bottom: 16px; line-height: 1.5; }
.add-exclusion-row { display: flex; gap: 8px; width: 100%; margin-bottom: 12px; }
.exclusion-input { flex: 1; background: var(--surface); border: 1px solid var(--border); border-radius: 8px; padding: 9px 12px; color: var(--text); font-size: 13px; outline: none; transition: border-color 0.2s; }
.exclusion-input:focus { border-color: var(--green); }
.add-btn { padding: 9px 16px; background: var(--green); color: #0a1a04; border: none; border-radius: 8px; font-weight: 600; cursor: pointer; font-size: 13px; }
.exclusions-list { width: 100%; }
.exclusion-item { display: flex; align-items: center; justify-content: space-between; padding: 10px 14px; background: var(--surface); border-radius: 8px; margin-bottom: 6px; border: 1px solid var(--border); }
.exclusion-name { font-size: 13px; font-family: monospace; }
.remove-btn { background: none; border: none; color: var(--muted); font-size: 18px; cursor: pointer; line-height: 1; padding: 0 4px; }
.remove-btn:hover { color: var(--red); }

/* ── Account ── */
.account-avatar { width: 64px; height: 64px; border-radius: 50%; background: var(--green-dim); color: var(--green); display: flex; align-items: center; justify-content: center; font-size: 26px; font-weight: 700; margin: 16px 0 10px; }
.account-email { font-size: 15px; font-weight: 500; margin-bottom: 10px; }
.account-badge { font-size: 11px; font-weight: 700; letter-spacing: 0.1em; padding: 4px 12px; border-radius: 20px; margin-bottom: 20px; }
.badge-premium { background: rgba(255,193,7,0.15); color: #ffc107; border: 1px solid rgba(255,193,7,0.3); }
.badge-free    { background: rgba(139,143,168,0.15); color: var(--muted); border: 1px solid var(--border); }
.account-cards { display: flex; gap: 10px; width: 100%; margin-bottom: 8px; }
.account-card { flex: 1; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius); padding: 12px 14px; display: flex; flex-direction: column; gap: 4px; }
.card-label { font-size: 11px; color: var(--muted); text-transform: uppercase; letter-spacing: 0.06em; }
.card-value { font-size: 14px; font-weight: 600; }
.settings-group { width: 100%; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius); overflow: hidden; }
.setting-row { display: flex; align-items: center; justify-content: space-between; padding: 13px 16px; border-bottom: 1px solid var(--border); gap: 12px; }
.setting-row:last-child { border-bottom: none; }
.setting-info { display: flex; flex-direction: column; gap: 2px; flex: 1; }
.setting-name { font-size: 13px; font-weight: 500; }
.setting-desc { font-size: 11px; color: var(--muted); }
.setting-select { background: var(--surface2); border: 1px solid var(--border); border-radius: 6px; padding: 5px 8px; color: var(--text); font-size: 12px; outline: none; cursor: pointer; }
.toggle-btn { width: 40px; height: 22px; border-radius: 11px; background: var(--surface2); border: 1px solid var(--border); cursor: pointer; position: relative; flex-shrink: 0; transition: background 0.2s, border-color 0.2s; }
.toggle-btn.toggle-on { background: var(--green-dim); border-color: var(--green); }
.toggle-knob { position: absolute; top: 2px; left: 2px; width: 16px; height: 16px; border-radius: 50%; background: var(--muted); transition: transform 0.2s, background 0.2s; }
.toggle-btn.toggle-on .toggle-knob { transform: translateX(18px); background: var(--green); }
.logout-btn { width: 100%; margin-top: 20px; padding: 12px; background: rgba(255,107,107,0.08); border: 1px solid rgba(255,107,107,0.25); border-radius: var(--radius); color: var(--red); font-size: 14px; font-weight: 600; cursor: pointer; transition: background 0.2s; }
.logout-btn:hover { background: rgba(255,107,107,0.15); }
.secondary-btn { padding: 10px 20px; background: var(--surface); border: 1px solid var(--border); border-radius: 8px; color: var(--text); font-size: 13px; cursor: pointer; }
.not-logged-in { display: flex; flex-direction: column; align-items: center; gap: 10px; padding: 40px 20px; color: var(--muted); text-align: center; }
.hint-text { font-size: 12px; }
.hint-text code { background: var(--surface2); padding: 2px 6px; border-radius: 4px; font-family: monospace; }

/* ── Loading ── */
.loading-state { display: flex; flex-direction: column; align-items: center; gap: 12px; padding: 40px; color: var(--muted); font-size: 13px; }
.spinner { width: 24px; height: 24px; border: 2px solid var(--border); border-top-color: var(--green); border-radius: 50%; animation: spin 0.8s linear infinite; }
.empty-state { padding: 30px 16px; text-align: center; color: var(--muted); font-size: 13px; }

/* ── Bottom nav ── */
.bottom-nav { display: flex; background: var(--surface); border-top: 1px solid var(--border); flex-shrink: 0; }
.nav-btn { flex: 1; display: flex; flex-direction: column; align-items: center; gap: 4px; padding: 10px 0 8px; background: none; border: none; color: var(--muted); cursor: pointer; font-size: 10px; font-weight: 500; transition: color 0.2s; letter-spacing: 0.03em; }
.nav-btn:hover { color: var(--text); }
.nav-btn.active { color: var(--green); }
</style>
