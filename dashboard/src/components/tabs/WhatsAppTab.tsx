import type { ProfileConfig } from '../../types'

interface Props {
  config: ProfileConfig
  onChange: (config: ProfileConfig) => void
}

export default function WhatsAppTab({ config, onChange }: Props) {
  const channel = config.channels.find((c) => c.type === 'whatsapp')
  const enabled = !!channel

  const toggle = () => {
    if (enabled) {
      onChange({ ...config, channels: config.channels.filter((c) => c.type !== 'whatsapp') })
    } else {
      onChange({
        ...config,
        channels: [...config.channels, { type: 'whatsapp', bridge_url: 'ws://localhost:3001' }],
      })
    }
  }

  const updateBridgeUrl = (v: string) => {
    const channels = config.channels.map((c) =>
      c.type === 'whatsapp' ? { ...c, bridge_url: v } : c
    )
    onChange({ ...config, channels })
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
        <span className="text-sm text-gray-300">Enable WhatsApp channel</span>
      </label>

      {enabled && (
        <div>
          <label className="block text-sm font-medium text-gray-300 mb-1.5">Bridge URL</label>
          <input
            value={(channel as any)?.bridge_url || 'ws://localhost:3001'}
            onChange={(e) => updateBridgeUrl(e.target.value)}
            placeholder="ws://localhost:3001"
            className="input text-xs font-mono"
          />
          <p className="text-[10px] text-gray-600 mt-1">
            WhatsApp Web bridge WebSocket URL (whatsmeow)
          </p>
        </div>
      )}
    </div>
  )
}
