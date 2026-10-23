# Incidente de suspensión: 17-10-2026

**Registro histórico, fuera de los pendientes del proyecto.** El usuario delimitó
expresamente que la investigación general de fallos de suspensión de Linux no
corresponde a Linux ScrollMaster. La prueba posterior con Linux ScrollMaster abierta pasó, contrastada con
journal y hardware; la aceptación de reanudación está completada. No se modificó
el sistema ni se requiere investigar el primer intento para continuar.

Los apartados siguientes preservan las observaciones de la sesión, sin constituir
un plan de trabajo vigente.

## Resultado

Durante la aceptación del hito de fiabilidad, con Linux ScrollMaster abierta, el usuario suspendió
el PC. La pantalla quedó negra y no consiguió reanudarlo. Apagó físicamente el equipo
y reinició. **Prueba fallida; reanudación de Linux ScrollMaster no verificable en este intento.**
No se repite la suspensión. No se atribuye la causa a Linux ScrollMaster, NVIDIA, firmware o kernel
sin evidencia suficiente. Las demás pruebas físicas se detienen para no continuar
sobre una sesión de aceptación interrumpida por este incidente.

## Evidencia de solo lectura

Boot afectado: `148d7f02a5314a5bb156e8dc6cd2ce96`.
Horario local America/Santiago (-03):

| Hora | Observación del journal |
|---|---|
| 22:16:22 | logind recibe solicitud de suspensión de systemctl |
| 22:16:23 | Inicia nvidia-suspend.service |
| 22:16:27 | nvidia-suspend.service finaliza correctamente |
| 22:16:27 | Inicia systemd-suspend.service |
| 22:16:27 | systemd-sleep informa sesiones de usuario no congeladas por variable explícita |
| 22:16:28 | systemd-sleep anuncia operación suspend |
| 22:16:28 | Kernel: PM: suspend entry (deep) |
| 22:20:15 | Primer registro del nuevo arranque |

En la selección consultada no hay mensaje de retorno ni ejecución de nvidia-resume.
La ausencia de registros no demuestra que el kernel nunca reanudara: puede haber
un bloqueo temprano, fallo de vídeo/wakeup o pérdida de mensajes sin persistir.
No se obtuvo traza de fallo que señale a Linux ScrollMaster. Tampoco hay una comparación de
suspensión con y sin Linux ScrollMaster que permita excluir su participación.

Estado leído tras el reinicio, no cambiado:
- Kernel: `7.2.5-3-omarchy`.
- Módulo NVIDIA: `580.178.04`.
- `/sys/power/mem_sleep`: `s2idle [deep]`.
- nvidia-suspend.service y nvidia-resume.service habilitados.
- systemd-suspend.service tiene drop-in distribuido en
  `/usr/lib/systemd/system/systemd-suspend.service.d/10-nvidia-no-freeze-session.conf`.
- Environment actual: `SYSTEMD_SLEEP_FREEZE_USER_SESSIONS=false`. El journal del
  intento también registra la exclusión de congelación; es configuración preexistente.

## Alcance cerrado

Solo se leyeron journal y estado de servicios/kernel. No se ejecutó suspend desde
el agente, ni sudo, ni se editaron archivos de sistema/compositor/controlador.
No hay tareas de diagnóstico ni modificaciones de sistema pendientes en Linux ScrollMaster
por este registro.

## Segundo intento informado por el usuario

El usuario informó después que volvió a suspender el PC y despertó correctamente.
Fue una acción del usuario, no una nueva prueba solicitada o ejecutada por el agente.
Esto muestra que el fallo no se reproduce en todos los intentos; no determina su causa.
Se preguntó si Linux ScrollMaster estaba abierta en ese segundo intento. Hasta aclararlo, no se
considera una comparación con/sin Linux ScrollMaster ni una validación de su recuperación.

## Nueva prueba autorizada

El usuario pidió explícitamente «probemos de nuevo» tras informar una suspensión
correcta. Se prepara un intento con Linux ScrollMaster abierta; no se cambia configuración del
sistema y la suspensión la iniciará el usuario. Antes de abrir Linux ScrollMaster, a 01:25:03 UTC,
una lectura real mostró 1000 DPI e inversión lateral false; el archivo persistente
conservaba «Mi perfil» (1850 DPI, lateral true) habilitado. No había instancia de
Linux ScrollMaster ejecutándose al comprobar los procesos. Se abrió la compilación existente
y se verificará la aplicación del perfil antes de autorizar el siguiente paso manual.
La condición de Linux ScrollMaster en la suspensión exitosa anterior sigue sin confirmarse.

Antes del nuevo intento: a 01:25:36 UTC, la lectura independiente confirmó que abrir
Linux ScrollMaster recuperó el perfil desde disco: DPI 1000 → 1850 e inversión lateral false → true.
Los seis ajustes quedaron disponibles y coincidentes con «Mi perfil». El journal del
arranque actual confirma además el intento exitoso previo: suspensión a 22:23:55 -03,
retorno a 22:24:03 y finalización correcta de systemd-suspend/nvidia-resume a 22:24:06.
Se pidió al usuario un nuevo ciclo con Linux ScrollMaster abierta, sin Restaurar/Actualizar al volver.

## Resultado del intento con Linux ScrollMaster abierta

El usuario confirmó «funcionó correctamente». El journal del arranque actual registra:
- 22:26:06 -03: inicio y operación suspend.
- 22:27:04 -03: `System returned from sleep operation 'suspend'`.
- 22:27:07 -03: finalización correcta de systemd-suspend y nvidia-resume.

Lectura independiente 01:27:42 UTC (22:27:42 local): MX Master 3S por Bolt, batería
40 %, DPI 1850, SmartShift 10, modo 2, inversión vertical false, resolución false,
inversión lateral true. Todos disponibles, «Mi perfil» persistido y habilitado.
No se pulsó Restaurar/Actualizar según el procedimiento confirmado por el usuario.

Este intento valida continuidad/recuperación tras una suspensión con Linux ScrollMaster abierta.
No establece cuántos valores retuvo el firmware ni si hubo escrituras al despertar;
no se presenta como demostración de una reaplicación necesaria en este caso.
La prueba de recuperación de Linux ScrollMaster queda verificada; este historial no bloquea
el desarrollo ni genera responsabilidades adicionales para el proyecto.
