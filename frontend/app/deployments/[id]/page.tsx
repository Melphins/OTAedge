'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import Link from 'next/link';
import { useParams, useRouter } from 'next/navigation';
import { api, type Deployment, type DeploymentDeviceInfo, type Model } from '@/lib/api';

function statusClass(status: string) {
  switch (status.toLowerCase()) {
    case 'completed':
    case 'succeeded':
      return 'bg-green-100 text-green-800';
    case 'deploying':
    case 'deployed':
      return 'bg-blue-100 text-blue-800';
    case 'failed':
    case 'error':
      return 'bg-red-100 text-red-800';
    case 'pending':
      return 'bg-yellow-100 text-yellow-800';
    default:
      return 'bg-gray-100 text-gray-800';
  }
}

function formatDate(value?: string) {
  if (!value) return 'N/A';
  return new Date(value).toLocaleString();
}

function progress(deployment: Deployment) {
  const target = deployment.devices_target || 0;
  if (target <= 0) return 0;
  const finished = deployment.devices_succeeded || deployment.devices_deployed || 0;
  return Math.min(100, Math.round((finished / target) * 100));
}

export default function DeploymentDetailsPage() {
  const params = useParams<{ id: string }>();
  const router = useRouter();
  const [deployment, setDeployment] = useState<Deployment | null>(null);
  const [devices, setDevices] = useState<DeploymentDeviceInfo[]>([]);
  const [model, setModel] = useState<Model | null>(null);
  const [models, setModels] = useState<Model[]>([]);
  const [rollbackModelId, setRollbackModelId] = useState('');
  const [rollbacking, setRollbacking] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');

  const fetchDeploymentDetails = useCallback(async () => {
    const deploymentData = await api.deployments.get(params.id);
    const [deviceData, modelData, modelList] = await Promise.all([
      api.deployments.devices(params.id),
      api.models.get(deploymentData.model_id),
      api.models.list({ page: 1, per_page: 100 }),
    ]);

    return { deploymentData, deviceData, modelData, modelList };
  }, [params.id]);

  const applyDetails = useCallback((data: Awaited<ReturnType<typeof fetchDeploymentDetails>>) => {
    setDeployment(data.deploymentData);
    setDevices(data.deviceData);
    setModel(data.modelData);
    setModels(data.modelList);
    setRollbackModelId((current) => current || data.modelList.find((item) => item.id !== data.deploymentData.model_id)?.id || '');
  }, []);

  useEffect(() => {
    const token = localStorage.getItem('token');
    if (!token) {
      router.push('/login');
      return;
    }

    fetchDeploymentDetails()
      .then(applyDetails)
      .catch((err: Error) => setError(err.message))
      .finally(() => setLoading(false));
  }, [applyDetails, fetchDeploymentDetails, router]);

  const phaseSummaries = useMemo(() => {
    const summaries = new Map<number, { total: number; succeeded: number; failed: number; deployed: number }>();
    devices.forEach((device) => {
      const summary = summaries.get(device.phase) || { total: 0, succeeded: 0, failed: 0, deployed: 0 };
      summary.total += 1;
      if (device.status === 'succeeded') summary.succeeded += 1;
      if (device.status === 'failed') summary.failed += 1;
      if (device.status === 'deployed') summary.deployed += 1;
      summaries.set(device.phase, summary);
    });
    return Array.from(summaries.entries()).sort(([a], [b]) => a - b);
  }, [devices]);

  const rollbackDeployment = async () => {
    if (!deployment || !rollbackModelId) return;
    setRollbacking(true);
    setError('');
    try {
      const rollback = await api.deployments.rollback(deployment.id, rollbackModelId);
      router.push(`/deployments/${rollback.id}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to start rollback');
    } finally {
      setRollbacking(false);
    }
  };

  if (loading) {
    return <div className="min-h-screen flex items-center justify-center bg-gray-100">Loading deployment...</div>;
  }

  if (!deployment) {
    return (
      <div className="min-h-screen bg-gray-100 p-6">
        <div className="max-w-4xl mx-auto bg-white rounded-md shadow p-6">
          <p className="text-red-700">{error || 'Deployment not found'}</p>
          <Link href="/deployments" className="mt-4 inline-block text-indigo-600 hover:text-indigo-900">
            Back to Deployments
          </Link>
        </div>
      </div>
    );
  }

  const pct = progress(deployment);

  return (
    <div className="min-h-screen bg-gray-100">
      <main className="max-w-7xl mx-auto py-6 px-4">
        <div className="mb-6 flex items-center justify-between">
          <div>
            <Link href="/deployments" className="text-indigo-600 hover:text-indigo-900 text-sm font-medium">
              Back to Deployments
            </Link>
            <h1 className="mt-3 text-2xl font-bold text-gray-900">Deployment Details</h1>
          </div>
          <span className={`inline-flex rounded-full px-3 py-1 text-sm font-semibold ${statusClass(deployment.status)}`}>
            {deployment.status}
          </span>
        </div>

        {error && (
          <div className="bg-red-50 border border-red-200 rounded-md p-4 mb-6 text-sm text-red-700">
            {error}
          </div>
        )}

        <section className="bg-white shadow rounded-md p-6 mb-6">
          <div className="grid grid-cols-1 gap-6 md:grid-cols-4">
            <div>
              <dt className="text-sm font-medium text-gray-500">Model</dt>
              <dd className="mt-1 text-sm text-gray-900">
                {model ? `${model.name} v${model.version}` : deployment.model_id}
              </dd>
            </div>
            <div>
              <dt className="text-sm font-medium text-gray-500">Strategy</dt>
              <dd className="mt-1 text-sm text-gray-900">
                {deployment.rollout_strategy} · {deployment.rollout_percentage}%
              </dd>
            </div>
            <div>
              <dt className="text-sm font-medium text-gray-500">Created</dt>
              <dd className="mt-1 text-sm text-gray-900">{formatDate(deployment.created_at)}</dd>
            </div>
            <div>
              <dt className="text-sm font-medium text-gray-500">Completed</dt>
              <dd className="mt-1 text-sm text-gray-900">{formatDate(deployment.completed_at)}</dd>
            </div>
          </div>

          <div className="mt-6">
            <div className="flex items-center justify-between text-sm text-gray-600">
              <span>
                {deployment.devices_succeeded || 0} succeeded · {deployment.devices_failed || 0} failed · {deployment.devices_target || 0} target
              </span>
              <span>{pct}%</span>
            </div>
            <div className="mt-2 h-3 rounded-full bg-gray-200">
              <div className="h-3 rounded-full bg-indigo-600" style={{ width: `${pct}%` }} />
            </div>
          </div>

          {phaseSummaries.length > 0 && (
            <div className="mt-6 grid grid-cols-1 gap-3 md:grid-cols-3">
              {phaseSummaries.map(([phase, summary]) => {
                const phasePct = summary.total > 0 ? Math.round(((summary.succeeded + summary.failed) / summary.total) * 100) : 0;
                const active = deployment.current_phase === phase;
                return (
                  <div key={phase} className={`rounded-md border p-3 ${active ? 'border-indigo-300 bg-indigo-50' : 'border-gray-200 bg-gray-50'}`}>
                    <div className="flex items-center justify-between text-sm">
                      <span className="font-medium text-gray-900">Phase {phase}</span>
                      <span className="text-gray-600">{phasePct}%</span>
                    </div>
                    <div className="mt-2 h-2 rounded-full bg-gray-200">
                      <div className="h-2 rounded-full bg-indigo-600" style={{ width: `${phasePct}%` }} />
                    </div>
                    <p className="mt-2 text-xs text-gray-600">
                      {summary.succeeded} succeeded · {summary.failed} failed · {summary.deployed} deploying · {summary.total} target
                    </p>
                  </div>
                );
              })}
            </div>
          )}
        </section>

        <section className="bg-white shadow rounded-md p-6 mb-6">
          <div className="flex flex-col gap-4 md:flex-row md:items-end md:justify-between">
            <div>
              <h2 className="text-lg font-semibold text-gray-900">Rollback</h2>
              <p className="mt-1 text-sm text-gray-600">Create an immediate rollback deployment for the same target devices.</p>
            </div>
            <div className="flex w-full flex-col gap-3 sm:flex-row md:w-auto">
              <select
                value={rollbackModelId}
                onChange={(event) => setRollbackModelId(event.target.value)}
                className="min-w-64 rounded-md border border-gray-300 px-3 py-2 text-sm text-gray-900 focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
              >
                <option value="">Select rollback model</option>
                {models
                  .filter((item) => item.id !== deployment.model_id)
                  .map((item) => (
                    <option key={item.id} value={item.id}>
                      {item.name} v{item.version} ({item.release_channel})
                    </option>
                  ))}
              </select>
              <button
                type="button"
                onClick={rollbackDeployment}
                disabled={!rollbackModelId || rollbacking}
                className="rounded-md bg-red-600 px-4 py-2 text-sm font-semibold text-white hover:bg-red-700 disabled:cursor-not-allowed disabled:bg-gray-300"
              >
                {rollbacking ? 'Starting...' : 'Rollback'}
              </button>
            </div>
          </div>
          {models.filter((item) => item.id !== deployment.model_id).length === 0 && (
            <p className="mt-3 text-sm text-gray-500">Upload another model version before rolling back.</p>
          )}
        </section>

        <section className="bg-white shadow rounded-md overflow-hidden">
          <div className="border-b border-gray-200 px-6 py-4">
            <h2 className="text-lg font-semibold text-gray-900">Target Devices</h2>
          </div>
          {devices.length === 0 ? (
            <div className="px-6 py-12 text-center text-sm text-gray-500">No target devices recorded</div>
          ) : (
            <div className="overflow-x-auto">
              <table className="min-w-full divide-y divide-gray-200">
                <thead className="bg-gray-50">
                  <tr>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">Device</th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">Status</th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">Phase</th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">Previous Model</th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">Target Model</th>
                  </tr>
                </thead>
                <tbody className="bg-white divide-y divide-gray-200">
                  {devices.map((device) => (
                    <tr key={device.device_id} className="hover:bg-gray-50">
                      <td className="px-6 py-4 whitespace-nowrap">
                        <div className="text-sm font-medium text-gray-900">{device.name}</div>
                        <div className="text-xs font-mono text-gray-500">{device.device_id}</div>
                      </td>
                      <td className="px-6 py-4 whitespace-nowrap">
                        <span className={`inline-flex rounded-full px-2 py-1 text-xs font-semibold ${statusClass(device.status)}`}>
                          {device.status}
                        </span>
                      </td>
                      <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">{device.phase}</td>
                      <td className="px-6 py-4 whitespace-nowrap text-xs font-mono text-gray-500">
                        {device.previous_model_id || 'N/A'}
                      </td>
                      <td className="px-6 py-4 whitespace-nowrap text-xs font-mono text-gray-500">
                        {device.current_model_id || 'N/A'}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </section>
      </main>
    </div>
  );
}
