FROM node:22-bookworm
WORKDIR /app
COPY apps/agent/package.json apps/agent/package-lock.json ./
RUN npm ci
COPY apps/agent/tsconfig.json ./
COPY apps/agent/src ./src
ENV NODE_ENV=production
EXPOSE 8790
CMD ["npm", "start"]
