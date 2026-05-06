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

PROJECT_DIR="/home/opc/hosting-projects/okru-twitch-scrape-id"
ERROR_DIR="${PROJECT_DIR}/error-docker"
DOCKER="/usr/bin/docker"
COMPOSE_CMD="sudo ${DOCKER} compose"

log "PROJECT_DIR: ${PROJECT_DIR}"
log "ERROR_DIR:   ${ERROR_DIR}"

# Crear carpeta de errores si no existe
mkdir -p "${ERROR_DIR}"

cd "${PROJECT_DIR}" || { log "ERROR: no se pudo hacer cd a ${PROJECT_DIR}"; exit 1; }

log "Leyendo servicios del compose..."

# Total de servicios definidos en el compose
TOTAL_SERVICES=$(${COMPOSE_CMD} config --services 2>/dev/null | wc -l | tr -d ' ')

log "Servicios definidos: ${TOTAL_SERVICES}"

if [ -z "${TOTAL_SERVICES}" ] || [ "${TOTAL_SERVICES}" -eq 0 ]; then
    log "ERROR: no se pudo leer el compose o no hay servicios definidos"
    TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
    touch "${ERROR_DIR}/compose_unreadable_${TIMESTAMP}.txt"
    exit 1
fi

# Contar contenedores realmente en estado "running" (grep sobre el output de ps)
PS_OUTPUT=$(${COMPOSE_CMD} ps 2>/dev/null)
log "Output de docker compose ps:"
if [ "${DEBUG}" = true ]; then echo "${PS_OUTPUT}"; fi
RUNNING_SERVICES=$(echo "${PS_OUTPUT}" | grep -cE "\bUp\b" || true)

log "Servicios corriendo: ${RUNNING_SERVICES}/${TOTAL_SERVICES}"

if [ "${RUNNING_SERVICES}" -lt "${TOTAL_SERVICES}" ]; then
    log "ALERTA: hay servicios caídos — ejecutando docker compose up -d"
    TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
    touch "${ERROR_DIR}/${TIMESTAMP}.txt"

    # Intentar levantar el compose automáticamente
    ${COMPOSE_CMD} up -d >> "${ERROR_DIR}/restart_${TIMESTAMP}.txt" 2>&1
    log "Resultado del restart guardado en: ${ERROR_DIR}/restart_${TIMESTAMP}.txt"
else
    log "OK: todos los servicios están corriendo"
fi
