'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { api, type Deployment, type Device, type Model } from '@/lib/api';

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

function deploymentProgress(deployment: Deployment) {
  const target = deployment.devices_target || 0;
  if (target <= 0) return 0;
  const finished = deployment.devices_succeeded || deployment.devices_deployed || 0;
  return Math.min(100, Math.round((finished / target) * 100));
}

export default function DeploymentsPage() {
  const router = useRouter();
  const [devices, setDevices] = useState<Device[]>([]);
  const [models, setModels] = useState<Model[]>([]);
  const [deployments, setDeployments] = useState<Deployment[]>([]);
  const [modelId, setModelId] = useState('');
  const [target, setTarget] = useState('all');
  const [rolloutStrategy, setRolloutStrategy] = useState('all_at_once');
  const [rolloutPercentage, setRolloutPercentage] = useState(100);
  const [loading, setLoading] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState('');

  const selectedDeviceId = target === 'all' ? undefined : target;

  const fetchDeploymentData = useCallback(async () => {
    const [deviceList, modelList, deploymentList] = await Promise.all([
      api.devices.list(),
      api.models.list({ page: 1, per_page: 100 }),
      api.deployments.list({ page: 1, per_page: 50 }),
    ]);

    return { deviceList, modelList, deploymentList };
  }, []);

  const applyDeploymentData = useCallback((data: Awaited<ReturnType<typeof fetchDeploymentData>>) => {
    setDevices(data.deviceList);
    setModels(data.modelList);
    setDeployments(data.deploymentList);
    setModelId((current) => current || data.modelList[0]?.id || '');
  }, []);

  useEffect(() => {
    const token = localStorage.getItem('token');
    if (!token) {
      router.push('/login');
      return;
    }

    fetchDeploymentData()
      .then(applyDeploymentData)
      .catch((err: Error) => setError(err.message))
      .finally(() => setLoading(false));
  }, [applyDeploymentData, fetchDeploymentData, router]);

  const modelsById = useMemo(() => {
    const lookup = new Map<string, Model>();
    models.forEach((model) => lookup.set(model.id, model));
    return lookup;
  }, [models]);

  const createDeployment = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!modelId) {
      setError('Select a model to deploy');
      return;
    }

    setSubmitting(true);
    setError('');
    try {
      await api.deployments.create({
        model_id: modelId,
        device_id: selectedDeviceId,
        rollout_strategy: rolloutStrategy,
        rollout_percentage: rolloutPercentage,
      });
      applyDeploymentData(await fetchDeploymentData());
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to create deployment');
    } finally {
      setSubmitting(false);
    }
  };

  const handleLogout = () => {
    localStorage.removeItem('token');
    router.push('/login');
  };

  if (loading) {
    return <div className="min-h-screen flex items-center justify-center bg-gray-100">Loading deployments...</div>;
  }

  return (
    <div className="min-h-screen bg-gray-100">
      <nav className="bg-white shadow">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="flex justify-between h-16">
            <div className="flex items-center space-x-8">
              <h1 className="text-xl font-semibold text-gray-900">OTAedge</h1>
              <Link href="/dashboard" className="text-gray-600 hover:text-gray-900 px-3 py-2 text-sm font-medium">
                Dashboard
              </Link>
              <Link href="/devices" className="text-gray-600 hover:text-gray-900 px-3 py-2 text-sm font-medium">
                Devices
              </Link>
              <Link href="/models" className="text-gray-600 hover:text-gray-900 px-3 py-2 text-sm font-medium">
                Models
              </Link>
              <Link href="/deployments" className="text-indigo-600 border-b-2 border-indigo-600 px-3 py-2 text-sm font-medium">
                Deployments
              </Link>
              <Link href="/alerts" className="text-gray-600 hover:text-gray-900 px-3 py-2 text-sm font-medium">
                Alerts
              </Link>
            </div>
            <button onClick={handleLogout} className="px-4 py-2 text-sm text-gray-700 hover:text-gray-900">
              Logout
            </button>
          </div>
        </div>
      </nav>

      <main className="max-w-7xl mx-auto py-6 px-4">
        <div className="mb-6">
          <h2 className="text-2xl font-bold text-gray-900">Deployments</h2>
          <p className="text-gray-600 mt-1">Create model deployments and track rollout status</p>
        </div>

        {error && (
          <div className="bg-red-50 border border-red-200 rounded-md p-4 mb-6 text-sm text-red-700">
            {error}
          </div>
        )}

        <section className="bg-white shadow rounded-md p-6 mb-6">
          <form onSubmit={createDeployment} className="grid grid-cols-1 lg:grid-cols-[1fr_1fr_180px_140px] gap-4 items-end">
            <label className="block">
              <span className="block text-sm font-medium text-gray-700 mb-1">Model</span>
              <select
                value={modelId}
                onChange={(event) => setModelId(event.target.value)}
                className="block w-full rounded-md border border-gray-300 px-3 py-2 text-sm text-gray-900"
              >
                {models.map((model) => (
                  <option key={model.id} value={model.id}>
                    {model.name} v{model.version} ({model.model_format.toUpperCase()})
                  </option>
                ))}
              </select>
            </label>

            <label className="block">
              <span className="block text-sm font-medium text-gray-700 mb-1">Target</span>
              <select
                value={target}
                onChange={(event) => setTarget(event.target.value)}
                className="block w-full rounded-md border border-gray-300 px-3 py-2 text-sm text-gray-900"
              >
                <option value="all">All devices</option>
                {devices.map((device) => (
                  <option key={device.id} value={device.id}>
                    {device.name} ({device.status})
                  </option>
                ))}
              </select>
            </label>

            <label className="block">
              <span className="block text-sm font-medium text-gray-700 mb-1">Strategy</span>
              <select
                value={rolloutStrategy}
                onChange={(event) => setRolloutStrategy(event.target.value)}
                className="block w-full rounded-md border border-gray-300 px-3 py-2 text-sm text-gray-900"
              >
                <option value="all_at_once">All at once</option>
                <option value="phased">Phased</option>
              </select>
            </label>

            <label className="block">
              <span className="block text-sm font-medium text-gray-700 mb-1">Percent</span>
              <input
                type="number"
                min="1"
                max="100"
                value={rolloutPercentage}
                onChange={(event) => setRolloutPercentage(Number(event.target.value))}
                className="block w-full rounded-md border border-gray-300 px-3 py-2 text-sm text-gray-900"
              />
            </label>

            <div className="lg:col-span-4 flex justify-end">
              <button
                type="submit"
                disabled={submitting || !modelId || devices.length === 0}
                className="inline-flex justify-center rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700 disabled:cursor-not-allowed disabled:bg-gray-400"
              >
                {submitting ? 'Deploying...' : 'Create Deployment'}
              </button>
            </div>
          </form>
          {(models.length === 0 || devices.length === 0) && (
            <p className="mt-4 text-sm text-gray-500">
              Deployments require at least one model and one registered device.
            </p>
          )}
        </section>

        <section className="bg-white shadow rounded-md overflow-hidden">
          <div className="border-b border-gray-200 px-6 py-4">
            <h3 className="text-lg font-semibold text-gray-900">Recent Deployments</h3>
          </div>
          {deployments.length === 0 ? (
            <div className="px-6 py-12 text-center text-sm text-gray-500">No deployments yet</div>
          ) : (
            <div className="overflow-x-auto">
              <table className="min-w-full divide-y divide-gray-200">
                <thead className="bg-gray-50">
                  <tr>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">Model</th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">Status</th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">Progress</th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">Strategy</th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">Created</th>
                    <th className="px-6 py-3 text-right text-xs font-medium text-gray-500 uppercase">Actions</th>
                  </tr>
                </thead>
                <tbody className="bg-white divide-y divide-gray-200">
                  {deployments.map((deployment) => {
                    const model = modelsById.get(deployment.model_id);
                    const progress = deploymentProgress(deployment);
                    return (
                      <tr key={deployment.id} className="hover:bg-gray-50">
                        <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-900">
                          {model ? `${model.name} v${model.version}` : deployment.model_id}
                        </td>
                        <td className="px-6 py-4 whitespace-nowrap">
                          <span className={`inline-flex rounded-full px-2 py-1 text-xs font-semibold ${statusClass(deployment.status)}`}>
                            {deployment.status}
                          </span>
                        </td>
                        <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                          <div className="flex items-center gap-3">
                            <div className="h-2 w-28 rounded-full bg-gray-200">
                              <div className="h-2 rounded-full bg-indigo-600" style={{ width: `${progress}%` }} />
                            </div>
                            <span>{progress}%</span>
                          </div>
                        </td>
                        <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                          {deployment.rollout_strategy} · {deployment.rollout_percentage}%
                        </td>
                        <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                          {formatDate(deployment.created_at)}
                        </td>
                        <td className="px-6 py-4 whitespace-nowrap text-right text-sm">
                          <Link href={`/deployments/${deployment.id}`} className="text-indigo-600 hover:text-indigo-900">
                            Details
                          </Link>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
        </section>
      </main>
    </div>
  );
}
