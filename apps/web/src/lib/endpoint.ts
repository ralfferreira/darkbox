import type { EndpointDto } from '../api/types';

export function endpointUrl(endpoint: EndpointDto): string {
  const defaultPort =
    (endpoint.scheme === 'http' && endpoint.port === 80) ||
    (endpoint.scheme === 'https' && endpoint.port === 443);
  return `${endpoint.scheme}://${endpoint.host}${defaultPort ? '' : `:${endpoint.port}`}${endpoint.path}`;
}

export function endpointRawUrl(endpoint: EndpointDto): string {
  const base = endpointUrl(endpoint);
  if (endpoint.query_names.length === 0) return base;
  return `${base}?${endpoint.query_names.map((name) => `${name}=`).join('&')}`;
}
