const API_URL = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3000';

export interface Device {
  id: string;
  device_id: string;
  name: string;
  device_type?: string;
  status: string;
  last_seen?: string;
  created_at: string;
  current_model_id?: string;
  model_version?: string;
}

export interface Model {
  id: string;
  name: string;
  version: number;
  is_active: boolean;
  release_channel: string;
  file_name: string;
  file_size_bytes?: number;
  s3_key: string;
  hash_sha256: string;
  model_format: string;
  metadata: Record<string, unknown>;
  created_at: string;
  updated_at: string;
}

export interface PresignedHeader {
  name: string;
  value: string;
}

export interface PresignedModelUploadResponse {
  upload_url: string;
  method: string;
  headers: PresignedHeader[];
  expires_in_seconds: number;
  s3_key: string;
}

export interface PresignedModelDownloadResponse {
  download_url: string;
  method: string;
  headers: PresignedHeader[];
  expires_in_seconds: number;
}

export interface ModelComparison {
  base: Model;
  target: Model;
  changed_fields: string[];
  version_delta: number;
  file_size_delta_bytes?: number;
  same_hash: boolean;
  same_format: boolean;
  same_metadata: boolean;
}

export interface Deployment {
  id: string;
  device_id?: string;
  model_id: string;
  status: string;
  rollout_strategy: string;
  rollout_percentage: number;
  deployed_at?: string;
  completed_at?: string;
  created_at: string;
  current_phase?: number;
  devices_target?: number;
  devices_deployed?: number;
  devices_succeeded?: number;
  devices_failed?: number;
  rollback_of?: string;
}

export interface DeploymentDeviceInfo {
  device_id: string;
  name: string;
  status: string;
  phase: number;
  previous_model_id?: string;
  current_model_id?: string;
}

export interface Alert {
  id: string;
  severity: 'critical' | 'warning' | 'info';
  title: string;
  description: string;
  source: string;
  status: 'open' | 'acknowledged' | 'silenced' | 'closed';
  created_at: string;
  device_id?: string;
  deployment_id?: string;
}

async function request<T>(
  endpoint: string,
  options: RequestInit = {}
): Promise<T> {
  const token = typeof window !== 'undefined' ? localStorage.getItem('token') : null;
  const headers: HeadersInit = {
    'Content-Type': 'application/json',
    ...(token && { Authorization: `Bearer ${token}` }),
    ...options.headers,
  };

  const response = await fetch(`${API_URL}${endpoint}`, {
    ...options,
    headers,
  });

  if (!response.ok) {
    const error = await response.json().catch(() => ({ message: 'Request failed' }));
    throw new Error(error.message || error.error || 'Request failed');
  }

  if (response.status === 204) {
    return undefined as T;
  }

  return response.json();
}

export const api = {
  auth: {
    register: (data: { email: string; password: string; org_name: string }) =>
      request('/api/auth/register', { method: 'POST', body: JSON.stringify(data) }),
    login: (data: { email: string; password: string }) =>
      request('/api/auth/login', { method: 'POST', body: JSON.stringify(data) }),
    refresh: (data: { refresh_token: string }) =>
      request('/api/auth/refresh', { method: 'POST', body: JSON.stringify(data) }),
    logout: (data: { refresh_token: string }) =>
      request('/api/auth/logout', { method: 'POST', body: JSON.stringify(data) }),
  },
  devices: {
    list: () => request<Device[]>('/api/devices'),
    create: (data: { name: string; device_type?: string }) =>
      request<Device>('/api/devices', { method: 'POST', body: JSON.stringify(data) }),
  },
  models: {
    list: (params?: { page?: number; per_page?: number }) => {
      const searchParams = new URLSearchParams();
      if (params?.page) searchParams.set('page', String(params.page));
      if (params?.per_page) searchParams.set('per_page', String(params.per_page));
      const query = searchParams.toString();
      return request<Model[]>(`/api/models${query ? `?${query}` : ''}`);
    },
    get: (id: string) => request<Model>(`/api/models/${id}`),
    getActive: (params: { name: string; release_channel?: string }) => {
      const searchParams = new URLSearchParams({ name: params.name });
      if (params.release_channel) searchParams.set('release_channel', params.release_channel);
      return request<Model>(`/api/models/active?${searchParams.toString()}`);
    },
    compare: (params: { base_id: string; target_id: string }) => {
      const searchParams = new URLSearchParams(params);
      return request<ModelComparison>(`/api/models/compare?${searchParams.toString()}`);
    },
    activate: (id: string, data?: { release_channel?: string }) => request<Model>(`/api/models/${id}/activate`, {
      method: 'POST',
      body: JSON.stringify(data || {}),
    }),
    delete: (id: string) => request<void>(`/api/models/${id}`, { method: 'DELETE' }),
    presignUpload: (data: {
      name: string;
      version: number;
      file_name: string;
      file_size_bytes?: number;
      hash_sha256: string;
      model_format?: 'onnx' | 'tflite';
      metadata?: Record<string, unknown>;
      expires_in_seconds?: number;
    }) => request<PresignedModelUploadResponse>('/api/models/presign-upload', {
      method: 'POST',
      body: JSON.stringify(data),
    }),
    completeUpload: (data: {
      name: string;
      version: number;
      file_name: string;
      file_size_bytes?: number;
      s3_key: string;
      hash_sha256: string;
      model_format: 'onnx' | 'tflite';
      metadata?: Record<string, unknown>;
    }) => request<Model>('/api/models/complete-upload', {
      method: 'POST',
      body: JSON.stringify(data),
    }),
    presignDownload: (id: string) => request<PresignedModelDownloadResponse>(`/api/models/${id}/presign-download`, {
      method: 'POST',
    }),
    upload: (file: File, metadata: {
      name: string;
      version?: number;
      model_format?: 'onnx' | 'tflite';
      sha256?: string;
      input_shapes?: string;
      output_shapes?: string;
      classes?: string;
    }) => {
      const token = localStorage.getItem('token');
      const headers: HeadersInit = token ? { Authorization: `Bearer ${token}` } : {};
      const form = new FormData();
      form.append('file', file);
      form.append('name', metadata.name);
      if (metadata.version) form.append('version', String(metadata.version));
      if (metadata.model_format) form.append('model_format', metadata.model_format);
      if (metadata.sha256) form.append('sha256', metadata.sha256);
      if (metadata.input_shapes) form.append('input_shapes', metadata.input_shapes);
      if (metadata.output_shapes) form.append('output_shapes', metadata.output_shapes);
      if (metadata.classes) form.append('classes', metadata.classes);
      return fetch(`${API_URL}/api/models/upload`, {
        method: 'POST',
        headers,
        body: form,
      }).then(res => {
        if (!res.ok) {
          return res.json()
            .catch(() => ({ error: 'Upload failed' }))
            .then(error => {
              throw new Error(error.message || error.error || 'Upload failed');
            });
        }
        return res.json();
      });
    },
    downloadUrl: (id: string) => `${API_URL}/api/models/${id}/download`,
  },
  deployments: {
    list: (params?: { page?: number; per_page?: number }) => {
      const searchParams = new URLSearchParams();
      if (params?.page) searchParams.set('page', String(params.page));
      if (params?.per_page) searchParams.set('per_page', String(params.per_page));
      const query = searchParams.toString();
      return request<Deployment[]>(`/api/deployments${query ? `?${query}` : ''}`);
    },
    get: (id: string) => request<Deployment>(`/api/deployments/${id}`),
    devices: (id: string) => request<DeploymentDeviceInfo[]>(`/api/deployments/${id}/devices`),
    create: (data: {
      model_id: string;
      device_id?: string;
      rollout_strategy?: string;
      rollout_percentage?: number;
      rollout_config?: Record<string, unknown>;
    }) => request<Deployment>('/api/deployments', { method: 'POST', body: JSON.stringify(data) }),
    rollback: (deployment_id: string, model_id: string) =>
      request<Deployment>(`/api/deployments/${deployment_id}/rollback`, { method: 'POST', body: JSON.stringify({ model_id }) }),
  },
  alerts: {
    list: (limit: number = 50) => {
      const searchParams = new URLSearchParams({ limit: String(limit) });
      return request<Alert[]>(`/api/alerts?${searchParams.toString()}`);
    },
    acknowledge: (id: string) =>
      request<void>(`/api/alerts/${id}/acknowledge`, { method: 'POST', body: JSON.stringify({}) }),
    close: (id: string) =>
      request<void>(`/api/alerts/${id}/close`, { method: 'POST', body: JSON.stringify({}) }),
  },
};
