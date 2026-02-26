import type { ProfileConfig } from '../../types'

interface Props {
  config: ProfileConfig
  onChange: (config: ProfileConfig) => void
}

export default function TelegramTab({ config, onChange }: Props) {
  const channel = config.channels.find((c) => c.type === 'telegram')
  const enabled = !!channel

  const toggle = () => {
    if (enabled) {
      onChange({ ...config, channels: config.channels.filter((c) => c.type !== 'telegram') })
    } else {
      onChange({
        ...config,
        channels: [...config.channels, { type: 'telegram', token_env: 'TELEGRAM_BOT_TOKEN' }],
      })
    }
  }

  const updateTokenEnv = (v: string) => {
    const channels = config.channels.map((c) =>
      c.type === 'telegram' ? { ...c, token_env: v } : c
    )
    onChange({ ...config, channels })
  }

  const updateBotToken = (v: string) => {
    const newEnvVars = { ...config.env_vars }
    if (v) {
      newEnvVars['TELEGRAM_BOT_TOKEN'] = v
    } else {
      delete newEnvVars['TELEGRAM_BOT_TOKEN']
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
        <span className="text-sm text-gray-300">Enable Telegram channel</span>
      </label>

      {enabled && (
        <>
          <div>
            <label className="block text-sm font-medium text-gray-300 mb-1.5">Token Env Var</label>
            <input
              value={(channel as any)?.token_env || 'TELEGRAM_BOT_TOKEN'}
              onChange={(e) => updateTokenEnv(e.target.value)}
              placeholder="TELEGRAM_BOT_TOKEN"
              className="input text-xs font-mono"
            />
          </div>
          <div>
            <label className="block text-sm font-medium text-gray-300 mb-1.5">Bot Token</label>
            <input
              type="password"
              value={config.env_vars['TELEGRAM_BOT_TOKEN'] || ''}
              onChange={(e) => updateBotToken(e.target.value)}
              placeholder="123456:ABC-DEF..."
              className="input text-xs font-mono"
            />
          </div>
        </>
      )}
    </div>
  )
}
