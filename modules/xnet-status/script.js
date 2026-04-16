(function() {
  let cutoverTs = null;
  let publicIp = (typeof XNET_REMOTE_IP !== 'undefined' && XNET_REMOTE_IP) ? XNET_REMOTE_IP : null;

  async function loadConfig() {
    try {
      const resp = await fetch('/cutover-env');
      if (!resp.ok) throw new Error('HTTP ' + resp.status);
      const text = await resp.text();
      const vars = {};
      text.split('\n').forEach(line => {
        const [k, v] = line.split('=');
        if (k && v) vars[k.trim()] = v.trim();
      });
      if (vars.XNET_CUTOVER_TIMESTAMP) {
        cutoverTs = Number(BigInt(vars.XNET_CUTOVER_TIMESTAMP) / 1000000000n);
      }
      if (!publicIp && vars.XNET_PUBLIC_IP) {
        publicIp = vars.XNET_PUBLIC_IP;
      }
    } catch (e) {
      console.error('Failed to load /cutover-env:', e);
    }
  }

  var migrationData = null;

  function isMigrationComplete() {
    // Before cutover time, migration hasn't started
    if (cutoverTs && Math.floor(Date.now() / 1000) < cutoverTs) return true;
    // No migration data yet — still waiting for metrics to appear
    if (!migrationData) return false;
    // Has data and all complete
    if (migrationData.has_data && migrationData.all_complete) return true;
    // No data from prometheus but we're past cutover — still waiting
    if (!migrationData.has_data) return false;
    return false;
  }

  function computePhase(nowS) {
    if (!cutoverTs) return { phase: 'UNKNOWN', target: null, label: 'timestamp unavailable' };
    var teardownTs = cutoverTs + 4 * 3600;
    if (nowS < cutoverTs) {
      return { phase: 'AWAITING CUTOVER', target: cutoverTs, label: 'until v3 \u2192 d14n cutover' };
    } else if (!isMigrationComplete()) {
      return { phase: 'MIGRATING', target: null, label: 'migrating v3 data to d14n...' };
    } else if (nowS < teardownTs) {
      return { phase: 'D14N ACTIVE', target: teardownTs, label: 'until teardown' };
    } else {
      return { phase: 'TEARDOWN IMMINENT', target: null, label: 'teardown imminent' };
    }
  }

  function formatTime(seconds) {
    if (seconds <= 0) return '00:00:00';
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    const s = seconds % 60;
    return [h, m, s].map(v => String(v).padStart(2, '0')).join(':');
  }

  function updateCountdown() {
    var now = Math.floor(Date.now() / 1000);
    var info = computePhase(now);
    document.getElementById('phase').innerHTML = '\u2588 ' + info.phase + ' \u2588';
    document.getElementById('countdown-label').textContent = info.label;

    if (info.phase === 'MIGRATING') {
      // Show spinner instead of countdown
      var spinner = ['\u2800', '\u2801', '\u2803', '\u2807', '\u280f', '\u281f', '\u283f', '\u287f'];
      var frame = spinner[Math.floor(Date.now() / 200) % spinner.length];
      var pct = migrationData ? Math.floor(migrationData.min_percent) : 0;
      document.getElementById('countdown').textContent = frame + ' ' + pct + '%';
      updateMigrationDisplay();
    } else if (info.target) {
      var remaining = Math.max(0, info.target - now);
      document.getElementById('countdown').textContent = formatTime(remaining);
      hideMigrationDisplay();
    } else {
      document.getElementById('countdown').textContent = '00:00:00';
      hideMigrationDisplay();
    }
    updateTimeline(now);
  }

  function updateTimeline(nowS) {
    if (!cutoverTs) return;
    const provisionStart = cutoverTs - 4 * 3600;
    const teardownEnd = cutoverTs + 4 * 3600;
    const total = teardownEnd - provisionStart;
    const elapsed = Math.max(0, Math.min(total, nowS - provisionStart));
    const pct = (elapsed / total) * 100;
    document.getElementById('timeline-fill').style.width = pct + '%';

    const phases = [
      { id: 'tl-provision', threshold: 0 },
      { id: 'tl-v3', threshold: 10 },
      { id: 'tl-cutover', threshold: 50 },
      { id: 'tl-d14n', threshold: 55 },
      { id: 'tl-teardown', threshold: 95 }
    ];
    phases.forEach(p => {
      const el = document.getElementById(p.id);
      el.className = pct >= p.threshold ? 'active' : '';
    });
  }

  function formatTimezones() {
    if (!cutoverTs) return;
    const date = new Date(cutoverTs * 1000);
    const zones = [
      { id: 'tz-utc', tz: 'UTC' },
      { id: 'tz-est', tz: 'America/New_York' },
      { id: 'tz-cst', tz: 'America/Chicago' },
      { id: 'tz-pst', tz: 'America/Los_Angeles' },
      { id: 'tz-cet', tz: 'Europe/Berlin' },
      { id: 'tz-jst', tz: 'Asia/Tokyo' }
    ];
    zones.forEach(z => {
      const el = document.getElementById(z.id);
      el.textContent = date.toLocaleTimeString('en-US', {
        timeZone: z.tz, hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false
      });
    });
  }

  // Map service display names to Docker container names
  var serviceContainerMap = {
    'xmtpd': 'xnet-100',
    'node-go (v3)': 'xnet-node',
    'gateway': 'xnet-gateway',
    'validation': 'xnet-validation',
    'contracts': 'xnet-anvil',
    'history': 'xnet-history-server'
  };

  async function checkHealth() {
    try {
      var controller = new AbortController();
      var timeout = setTimeout(function() { controller.abort(); }, 5000);
      var resp = await fetch('/api/health', { signal: controller.signal });
      clearTimeout(timeout);
      if (!resp.ok) throw new Error('HTTP ' + resp.status);
      var containers = await resp.json();

      var rows = document.querySelectorAll('.service-status[data-port]');
      rows.forEach(function(row) {
        var serviceName = row.closest('.service-row').querySelector('.service-name').textContent;
        var containerName = serviceContainerMap[serviceName];
        var dot = row.querySelector('.dot');
        var text = row.querySelector('.status-text');

        if (containerName && containers[containerName]) {
          var info = containers[containerName];
          if (info.up) {
            dot.className = 'dot up';
            text.className = 'status-text up';
            text.textContent = 'UP';
          } else {
            dot.className = 'dot down';
            text.className = 'status-text down';
            text.textContent = info.state.toUpperCase();
          }
        } else {
          dot.className = 'dot down';
          text.className = 'status-text down';
          text.textContent = 'NOT FOUND';
        }
      });
    } catch (e) {
      console.error('Health check failed:', e);
    }
  }

  function populateIpFields() {
    var remoteDomain = (typeof XNET_REMOTE_DOMAIN !== 'undefined' && XNET_REMOTE_DOMAIN) ? XNET_REMOTE_DOMAIN : null;
    var ipHost = publicIp || 'localhost';

    document.getElementById('server-ip').textContent = publicIp || 'unknown';

    // Dashboard links — use subdomain.domain if available, else IP:port fallback
    document.querySelectorAll('.dashboard-link').forEach(function(el) {
      var subdomain = el.getAttribute('data-subdomain');
      var url;
      if (remoteDomain && subdomain) {
        url = 'http://' + subdomain + '.' + remoteDomain;
      } else {
        url = 'http://' + ipHost + ':' + (el.getAttribute('data-port') || '80');
      }
      el.href = url;
      el.target = '_blank';
      el.rel = 'noopener';
      el.textContent = url;
    });

    // Connection endpoints — use domain if available, else sslip.io
    var baseDomain;
    if (remoteDomain) {
      baseDomain = remoteDomain;
    } else if (publicIp) {
      baseDomain = publicIp.replace(/\./g, '-') + '.sslip.io';
    } else {
      baseDomain = 'localhost';
    }
    document.querySelectorAll('.endpoint-url').forEach(function(el) {
      var name = el.getAttribute('data-name');
      var port = el.getAttribute('data-port');
      if (name !== null) {
        el.textContent = 'http://' + (name ? name + '.' : '') + baseDomain;
      } else if (port) {
        el.textContent = 'http://' + ipHost + ':' + port;
      }
    });
  }

  window.copyEndpoint = function(btn) {
    const row = btn.closest('.endpoint-row');
    const url = row.querySelector('.endpoint-url').textContent;
    try {
      const textarea = document.createElement('textarea');
      textarea.value = url;
      textarea.style.position = 'fixed';
      textarea.style.opacity = '0';
      document.body.appendChild(textarea);
      textarea.select();
      document.execCommand('copy');
      document.body.removeChild(textarea);
      btn.classList.add('copied');
      btn.innerHTML = '&#10003;';
      setTimeout(() => {
        btn.classList.remove('copied');
        btn.innerHTML = '&#9112;';
      }, 2000);
    } catch (e) {
      console.error('Copy failed:', e);
    }
  };

  async function checkMigration() {
    try {
      var resp = await fetch('/api/migration');
      if (!resp.ok) return;
      migrationData = await resp.json();
    } catch (e) {
      // migration endpoint not available yet
    }
  }

  function updateMigrationDisplay() {
    var el = document.getElementById('migration-progress');
    if (!el) return;
    if (!migrationData || !migrationData.has_data) {
      el.style.display = 'block';
      el.textContent = '';
      var waitDiv = document.createElement('div');
      waitDiv.style.cssText = 'color:#555;font-size:11px;text-align:center;';
      waitDiv.textContent = 'Waiting for migration metrics...';
      el.appendChild(waitDiv);
      return;
    }
    el.style.display = 'block';
    el.textContent = '';
    var tables = migrationData.tables.sort(function(a, b) { return a.table.localeCompare(b.table); });
    for (var i = 0; i < tables.length; i++) {
      var t = tables[i];
      var pct = Math.min(100, Math.max(0, Math.floor(t.percent)));
      var filled = Math.round(pct / 5);
      var empty = 20 - filled;
      var bar = '\u2588'.repeat(filled) + '\u2591'.repeat(empty);
      var color = pct >= 100 ? '#0f0' : pct >= 50 ? '#ff0' : '#f66';
      var row = document.createElement('div');
      row.style.cssText = 'font-size:11px;margin:3px 0;font-family:monospace;';
      var nameSpan = document.createElement('span');
      nameSpan.style.cssText = 'color:#aaa;display:inline-block;min-width:160px;';
      nameSpan.textContent = t.table;
      var barSpan = document.createElement('span');
      barSpan.style.color = color;
      barSpan.textContent = '[' + bar + ']';
      var pctSpan = document.createElement('span');
      pctSpan.style.cssText = 'color:' + color + ';margin-left:6px;';
      pctSpan.textContent = pct + '%';
      row.appendChild(nameSpan);
      row.appendChild(document.createTextNode('  '));
      row.appendChild(barSpan);
      row.appendChild(pctSpan);
      el.appendChild(row);
    }
  }

  function hideMigrationDisplay() {
    var el = document.getElementById('migration-progress');
    if (el) el.style.display = 'none';
  }

  async function init() {
    await loadConfig();
    populateIpFields();
    formatTimezones();
    updateCountdown();
    setInterval(updateCountdown, 1000);
    checkHealth();
    setInterval(checkHealth, 30000);
    checkMigration();
    setInterval(checkMigration, 2000);
  }

  document.addEventListener('DOMContentLoaded', init);
})();
