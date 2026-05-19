#!/bin/bash

# ============================================================
# check_docker.sh
# Valida que el docker-compose esté completamente activo.
# Si algún contenedor no está corriendo, registra el error.
# Pensado para ejecutarse desde cron cada 2 horas.
# ============================================================

DEBUG=false
if [ "${1}" = "--debug" ]; then
    DEBUG=true
fi

log() {
    if [ "${DEBUG}" = true ]; then
        echo "[$(date '+%H:%M:%S')] $*"
    fi
}

PROJECT_DIR="/home/opc/hosting-projects/okru-scraping"
ERROR_DIR="${PROJECT_DIR}/error-docker"
DOCKER="/usr/bin/docker"
COMPOSE_CMD="sudo ${DOCKER} compose"

log "PROJECT_DIR: ${PROJECT_DIR}"
log "ERROR_DIR:   ${ERROR_DIR}"

# Crear carpeta de errores si no existe
mkdir -p "${ERROR_DIR}"

cd "${PROJECT_DIR}" || { log "ERROR: no se pudo hacer cd a ${PROJECT_DIR}"; exit 1; }

log "Leyendo servicios del compose..."

# Servicios activos (excluir profiles: debug)
TOTAL_SERVICES=$(${COMPOSE_CMD} config --services 2>/dev/null | grep -Ev '^debug$' | wc -l | tr -d ' ')

log "Servicios definidos (sin debug): ${TOTAL_SERVICES}"

if [ -z "${TOTAL_SERVICES}" ] || [ "${TOTAL_SERVICES}" -eq 0 ]; then
    log "ERROR: no se pudo leer el compose o no hay servicios definidos"
    TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
    touch "${ERROR_DIR}/compose_unreadable_${TIMESTAMP}.txt"
    exit 1
fi

# Verificar health del backend via HTTP
BACKEND_HEALTH=$(curl -sf http://localhost:9622/health 2>/dev/null)
if [ $? -eq 0 ]; then
    log "Backend health OK: ${BACKEND_HEALTH}"
else
    log "ALERTA: backend no responde en :9622"
fi

# Contar contenedores realmente en estado "running"
PS_OUTPUT=$(${COMPOSE_CMD} ps 2>/dev/null)
log "Output de docker compose ps:"
if [ "${DEBUG}" = true ]; then echo "${PS_OUTPUT}"; fi
RUNNING_SERVICES=$(echo "${PS_OUTPUT}" | grep -cE "\bUp\b|\brunning\b" || true)

log "Servicios corriendo: ${RUNNING_SERVICES}/${TOTAL_SERVICES}"

if [ "${RUNNING_SERVICES}" -lt "${TOTAL_SERVICES}" ]; then
    log "ALERTA: hay servicios caídos — ejecutando docker compose up -d"
    TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
    touch "${ERROR_DIR}/${TIMESTAMP}.txt"

    # Intentar levantar el compose automáticamente (sin profiles debug/legacy)
    ${COMPOSE_CMD} up -d >> "${ERROR_DIR}/restart_${TIMESTAMP}.txt" 2>&1
    log "Resultado del restart guardado en: ${ERROR_DIR}/restart_${TIMESTAMP}.txt"
else
    log "OK: todos los servicios están corriendo"
fi

