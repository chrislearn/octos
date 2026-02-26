import type { ProfileConfig } from '../../types'

interface Props {
  config: ProfileConfig
  onChange: (config: ProfileConfig) => void
}

export default function GatewayTab({ config, onChange }: Props) {
  const updateGateway = (field: string, value: number | string | null) => {
    onChange({
      ...config,
      gateway: { ...config.gateway, [field]: value },
    })
  }

  return (
    <div className="space-y-4">
      <div>
        <label className="block text-sm font-medium text-gray-300 mb-1.5">Max History</label>
        <input
          type="number"
          value={config.gateway.max_history ?? ''}
          onChange={(e) =>
            updateGateway('max_history', e.target.value ? Number(e.target.value) : null)
          }
          placeholder="50"
          className="input"
        />
        <p className="text-[10px] text-gray-600 mt-1">
          Maximum number of messages to keep in conversation history
        </p>
      </div>

      <div>
        <label className="block text-sm font-medium text-gray-300 mb-1.5">Max Iterations</label>
        <input
          type="number"
          value={config.gateway.max_iterations ?? ''}
          onChange={(e) =>
            updateGateway('max_iterations', e.target.value ? Number(e.target.value) : null)
          }
          placeholder="50"
          className="input"
        />
        <p className="text-[10px] text-gray-600 mt-1">
          Maximum tool-call iterations per agent turn
        </p>
      </div>

      <div>
        <label className="block text-sm font-medium text-gray-300 mb-1.5">System Prompt</label>
        <textarea
          value={config.gateway.system_prompt ?? ''}
          onChange={(e) => updateGateway('system_prompt', e.target.value || null)}
          placeholder="You are a helpful assistant."
          rows={4}
          className="input"
        />
        <p className="text-[10px] text-gray-600 mt-1">
          Custom system prompt for this gateway instance
        </p>
      </div>
    </div>
  )
}
