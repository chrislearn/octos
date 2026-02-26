import type { ProfileConfig } from '../../types'

interface Props {
  config: ProfileConfig
  onChange: (config: ProfileConfig) => void
}

export default function FeishuTab({ config, onChange }: Props) {
  const channel = config.channels.find((c) => c.type === 'feishu')
  const enabled = !!channel

  const toggle = () => {
    if (enabled) {
      onChange({ ...config, channels: config.channels.filter((c) => c.type !== 'feishu') })
    } else {
      onChange({
        ...config,
        channels: [
          ...config.channels,
          {
            type: 'feishu',
            app_id_env: 'FEISHU_APP_ID',
            app_secret_env: 'FEISHU_APP_SECRET',
            verification_token_env: 'FEISHU_VERIFICATION_TOKEN',
            encrypt_key_env: 'FEISHU_ENCRYPT_KEY',
          },
        ],
      })
    }
  }

  const updateField = (field: string, v: string) => {
    const channels = config.channels.map((c) =>
      c.type === 'feishu' ? { ...c, [field]: v } : c
    )
    onChange({ ...config, channels })
  }

  const updateEnv = (key: string, value: string) => {
    const newEnvVars = { ...config.env_vars }
    if (value) {
      newEnvVars[key] = value
    } else {
      delete newEnvVars[key]
    }
    onChange({ ...config, env_vars: newEnvVars })
  }

  return (
    <div className="space-y-4">
      <label className="flex items-center gap-2 cursor-pointer">
        <input
          type="checkbox"
          checked={enabled}
          onChange={toggle}
          className="w-4 h-4 rounded bg-surface-dark border-gray-600 text-accent focus:ring-accent"
        />
        <span className="text-sm text-gray-300">Enable Feishu / Lark channel</span>
      </label>

      {enabled && (
        <>
          <div>
            <label className="block text-sm font-medium text-gray-300 mb-1.5">App ID</label>
            <input
              type="password"
              value={config.env_vars['FEISHU_APP_ID'] || ''}
              onChange={(e) => updateEnv('FEISHU_APP_ID', e.target.value)}
              placeholder="cli_xxxx"
              className="input text-xs font-mono"
            />
            <p className="text-[10px] text-gray-600 mt-1">FEISHU_APP_ID</p>
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-300 mb-1.5">App Secret</label>
            <input
              type="password"
              value={config.env_vars['FEISHU_APP_SECRET'] || ''}
              onChange={(e) => updateEnv('FEISHU_APP_SECRET', e.target.value)}
              placeholder="secret..."
              className="input text-xs font-mono"
            />
            <p className="text-[10px] text-gray-600 mt-1">FEISHU_APP_SECRET</p>
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-300 mb-1.5">Verification Token</label>
            <input
              type="password"
              value={config.env_vars['FEISHU_VERIFICATION_TOKEN'] || ''}
              onChange={(e) => updateEnv('FEISHU_VERIFICATION_TOKEN', e.target.value)}
              placeholder="verification token"
              className="input text-xs font-mono"
            />
            <p className="text-[10px] text-gray-600 mt-1">FEISHU_VERIFICATION_TOKEN</p>
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-300 mb-1.5">Encrypt Key</label>
            <input
              type="password"
              value={config.env_vars['FEISHU_ENCRYPT_KEY'] || ''}
              onChange={(e) => updateEnv('FEISHU_ENCRYPT_KEY', e.target.value)}
              placeholder="encrypt key"
              className="input text-xs font-mono"
            />
            <p className="text-[10px] text-gray-600 mt-1">FEISHU_ENCRYPT_KEY</p>
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="block text-sm font-medium text-gray-300 mb-1.5">Region</label>
              <select
                value={(channel as any)?.region || 'feishu'}
                onChange={(e) => updateField('region', e.target.value)}
                className="input text-xs"
              >
                <option value="feishu">Feishu (China)</option>
                <option value="lark">Lark (International)</option>
              </select>
            </div>
            <div>
              <label className="block text-sm font-medium text-gray-300 mb-1.5">Webhook Port</label>
              <input
                type="number"
                value={(channel as any)?.webhook_port || ''}
                onChange={(e) => updateField('webhook_port', e.target.value ? Number(e.target.value) as any : '')}
                placeholder="9321"
                className="input text-xs"
              />
            </div>
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-300 mb-1.5">Mode</label>
            <select
              value={(channel as any)?.mode || 'webhook'}
              onChange={(e) => updateField('mode', e.target.value)}
              className="input text-xs"
            >
              <option value="webhook">Webhook (HTTP callback)</option>
              <option value="websocket">WebSocket (long-lived connection)</option>
            </select>
          </div>
        </>
      )}
    </div>
  )
}
