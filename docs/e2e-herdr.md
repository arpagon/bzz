# Pruebas E2E asistidas con Herdr

Esta guía explica cómo operar el [plan E2E manual](e2e-manual.md) desde un
panel de terminal administrado por Herdr. Herdr es opcional y no es una
dependencia de `bzz`.

Usa siempre:

- una identidad desechable;
- directorios aislados;
- un relay y un canal dedicados a pruebas;
- un binario `release`;
- placeholders en capturas y evidencias públicas.

Nunca incluyas en comandos, argumentos, variables ordinarias, logs o capturas
un `nsec`, una contraseña, una clave administrativa o el contenido de un
backup NIP-49.

## 1. Preparar el proceso

Crea `.env` desde la plantilla y rellena únicamente valores no secretos:

```bash
cp .env.sample .env
$EDITOR .env
set -a; source .env; set +a
cargo build --release --locked
```

Como mínimo, configura un relay de prueba:

```dotenv
BZZ_RELAY_URL=wss://relay.example
BZZ_E2E_ROOT=/tmp/bzz-e2e
BZZ_CONFIG_DIR=/tmp/bzz-e2e/config
BZZ_DATA_DIR=/tmp/bzz-e2e/data
BZZ_CACHE_DIR=/tmp/bzz-e2e/cache
```

Comprueba Herdr y enumera los paneles:

```bash
herdr status
herdr pane list
```

Elige un panel de terminal que no contenga otro agente y guarda su identificador:

```bash
export BZZ_PANE=w1:p2
herdr pane process-info --pane "$BZZ_PANE"
```

Los IDs como `w1:p2` son ejemplos locales de Herdr, no identificadores de Buzz.

## 2. Lanzar y observar bzz

Ejecuta el proceso dentro del panel:

```bash
herdr pane run "$BZZ_PANE" \
  'set -a; source .env; set +a; "$BZZ_BIN"'
```

Lee la pantalla visible sin adjuntarte al terminal:

```bash
herdr pane read "$BZZ_PANE" --source visible --format text
```

Espera un estado concreto cuando quieras sincronizar un script con la TUI:

```bash
herdr pane wait-output "$BZZ_PANE" \
  --source visible \
  --match 'NORMAL · online' \
  --timeout 15000
```

Confirma qué proceso ocupa el panel:

```bash
herdr pane process-info --pane "$BZZ_PANE"
```

## 3. Enviar teclas

Usa `send-keys` para manejar la TUI:

```bash
herdr pane send-keys "$BZZ_PANE" 'shift+?'
sleep 0.3
herdr pane send-keys "$BZZ_PANE" esc
sleep 0.3
herdr pane send-keys "$BZZ_PANE" 'shift+q'
```

Convenciones útiles:

| Acción bzz | Tecla Herdr |
|---|---|
| Ayuda `?` | `shift+?` |
| Salir `Q` | `shift+q` |
| Borrar `D` | `shift+d` |
| No leído `U` | `shift+u` |
| Final `G` | `shift+g` |
| Finder | `ctrl+p` |
| Thread | `ctrl+]` o `enter` |
| Escape | `esc` |
| Confirmar | `enter` |

Envía `esc` en una llamada separada y espera antes de enviar una letra. Si
Herdr escribe `esc` y una letra sin pausa, el terminal puede interpretarlos
como `Alt+letra`.

No uses `herdr pane send-text` para manejar los modos de `bzz`: Herdr puede
envolverlo como bracketed paste y la TUI procesa eventos de teclado. Usa
`send-keys`.

### Texto ASCII de prueba

Para mensajes E2E sencillos puedes convertir texto ASCII en teclas:

```bash
herdr_type_ascii() {
  local pane=$1
  local text=$2
  local -a keys
  mapfile -t keys < <(python3 - "$text" <<'PY'
import sys
for char in sys.argv[1]:
    if char.isupper():
        print("shift+" + char.lower())
    elif char == " ":
        print("space")
    else:
        print(char)
PY
  )
  herdr pane send-keys "$pane" "${keys[@]}"
}

message="E2E-basic-$(date -u +%Y%m%dT%H%M%SZ)"
herdr pane send-keys "$BZZ_PANE" i
sleep 0.3
herdr_type_ascii "$BZZ_PANE" "$message"
```

Lee la pantalla y comprueba el texto antes de enviarlo:

```bash
herdr pane read "$BZZ_PANE" --source visible --format text
herdr pane send-keys "$BZZ_PANE" enter
```

Usa contenido sin información personal y un canal E2E dedicado.

## 4. Prompts secretos

Herdr puede lanzar comandos que solicitan contraseñas, pero el operador debe
escribir o pegar el secreto directamente en el terminal con el prompt sin eco.

Ejemplo:

```bash
herdr pane run "$BZZ_PANE" \
  '"$BZZ_BIN" identity backup "$BZZ_IDENTITY_ID" --output "$BZZ_E2E_ROOT/identity.ncryptsec"'
```

Cuando aparezca `New backup passphrase:`:

1. enfoca el panel visualmente;
2. escribe la contraseña o pégala desde un gestor de credenciales;
3. pulsa `Enter`;
4. repítela solo si el prompt pide confirmación.

Si utilizas `gopass`, copia sin imprimir:

```bash
gopass show --clip <entrada-del-backup>
```

No automatices secretos mediante:

- `herdr pane send-text`;
- `herdr pane send-keys`;
- argumentos de proceso;
- variables de entorno ordinarias;
- archivos `.env`;
- sustitución `$(gopass ...)`;
- salida o historial del terminal.

El contenido del prompt no tiene eco, pero revisa igualmente cualquier captura
antes de conservarla.

## 5. Recorrido básico con Herdr

Correspondencia con los bloques 0–4 del plan manual:

1. Ejecuta versión, paths y check con `herdr pane run`.
2. Inicia `bzz`, abre ayuda con `shift+?`, cierra con `esc` y sal con
   `shift+q`.
3. Crea la identidad desde el shell del panel; completa los prompts secretos
   manualmente.
4. Configura una comunidad y un canal exclusivamente E2E.
5. Inicia la TUI y espera `NORMAL · online`.
6. Entra al canal con `enter`.
7. Abre el compositor con `i`, escribe un marcador único y envía con `enter`.
8. Lee la pantalla hasta que desaparezca `[pending]`.
9. Sal, reinicia y confirma que el mensaje aparece una sola vez.

Ejemplo de comprobación visual:

```bash
herdr pane read "$BZZ_PANE" --source visible --format text \
  | grep -F "$message"
```

La salida visible es evidencia auxiliar; SQLite y el ACK del relay siguen
siendo las fuentes para comprobar deduplicación y aceptación.

## 6. Conversación y recuperación

Secuencias comunes, dejando una pausa al cambiar de modo:

```bash
# Draft
herdr pane send-keys "$BZZ_PANE" i
# escribir texto con herdr_type_ascii
herdr pane send-keys "$BZZ_PANE" esc
sleep 0.3
herdr pane send-keys "$BZZ_PANE" i

# Reacción seleccionada
herdr pane send-keys "$BZZ_PANE" r
sleep 0.3
herdr pane send-keys "$BZZ_PANE" enter

# Borrado propio
herdr pane send-keys "$BZZ_PANE" 'shift+d'
sleep 0.3
herdr pane send-keys "$BZZ_PANE" y

# Marcar no leído y volver al final
herdr pane send-keys "$BZZ_PANE" 'shift+u'
herdr pane send-keys "$BZZ_PANE" 'shift+g'

# Bloqueo del proceso
herdr pane send-keys "$BZZ_PANE" : l o c k enter
```

Para `identity missing`, elimina únicamente la credencial de la identidad E2E
desde el gestor del sistema, nunca su configuración. Verifica en Herdr:

- historial visible;
- estado `identity missing`;
- ninguna conexión del proceso;
- `i`, `r` y `D` bloqueados;
- outbox sin cambios.

Después sal con `shift+q`, lanza `identity restore-backup`, completa la
contraseña manualmente y vuelve a esperar `NORMAL · online`.

### Temas

El picker también puede probarse sin datos específicos del relay:

```bash
herdr pane send-keys "$BZZ_PANE" ctrl+y
sleep 0.3
herdr_type_ascii "$BZZ_PANE" "nord"
herdr pane read "$BZZ_PANE" --source visible --format text
herdr pane send-keys "$BZZ_PANE" down
sleep 0.3
herdr pane send-keys "$BZZ_PANE" esc
```

Comprueba que `Esc` restaura el buffer con el tema previo. Reabre el picker,
usa `tab` para alternar alcance y confirma con `enter`; reinicia `bzz` para
verificar persistencia. No guardes capturas que contengan nombres o contenido
de comunidades reales.

## 7. Segundo cliente independiente

No ejecutes dos clientes contra el mismo archivo SQLite para una prueba de
convergencia real. Crea otra raíz y una copia consistente:

```bash
export BZZ_SECOND_ROOT=/tmp/bzz-e2e-client2
mkdir -p "$BZZ_SECOND_ROOT"/{config,data,cache}
cp "$BZZ_CONFIG_DIR/config.toml" "$BZZ_SECOND_ROOT/config/config.toml"
sqlite3 "$BZZ_DATA_DIR/bzz.db" \
  ".backup '$BZZ_SECOND_ROOT/data/bzz.db'"
```

Elimina de **la copia** los slots locales para que el segundo cliente genere
otro `client_id`:

```bash
sqlite3 "$BZZ_SECOND_ROOT/data/bzz.db" 'DELETE FROM read_slots;'
```

Crea un panel temporal:

```bash
herdr pane split "$BZZ_PANE" \
  --direction down \
  --ratio 0.45 \
  --cwd "$PWD" \
  --focus
herdr pane list
export BZZ_SECOND_PANE=w1:p3
```

Inicia el segundo cliente con rutas distintas:

```bash
herdr pane run "$BZZ_SECOND_PANE" \
  'set -a; source .env; set +a; BZZ_CONFIG_DIR=/tmp/bzz-e2e-client2/config BZZ_DATA_DIR=/tmp/bzz-e2e-client2/data BZZ_CACHE_DIR=/tmp/bzz-e2e-client2/cache "$BZZ_BIN"'
```

Comprueba:

- ambos paneles en `online`;
- mensajes A→B y B→A;
- una sola copia por base;
- dos `client_id` diferentes;
- el mismo máximo `read_at`;
- el slot propio con `is_local=1` y el otro con `is_local=0` en cada base.

Cierra y elimina solo los recursos temporales:

```bash
herdr pane send-keys "$BZZ_SECOND_PANE" 'shift+q'
herdr pane close "$BZZ_SECOND_PANE"
rm -rf "$BZZ_SECOND_ROOT"
```

## 8. Caché offline sin cortar la red del host

No desconectes el host ni bloquees un relay compartido. Copia el estado a otra
raíz desechable, edita únicamente esa copia y cambia el relay a un puerto
loopback cerrado:

```toml
relay_url = "ws://127.0.0.1:9/"
allow_insecure_localhost = true
```

Lanza esa copia con overrides de paths. El resultado esperado es:

- mensajes cacheados visibles;
- estado `offline cache`;
- error de conexión no fatal;
- `:reconnect` reintenta sin perder historial.

Cierra el proceso, borra la copia y vuelve a iniciar el estado original para
confirmar `online`.

## 9. Evidencia y limpieza

Puedes guardar únicamente salida ya revisada:

```bash
herdr pane read "$BZZ_PANE" --source visible --format text \
  > /tmp/bzz-e2e-screen.txt
```

Antes de adjuntarla, elimina:

- hosts, nombres o identificadores de comunidades reales;
- pubkeys que no sean fixtures públicos;
- nombres y contenido de usuarios reales;
- rutas privadas;
- cualquier material de autenticación.

No guardes como evidencia `.env`, backups `ncryptsec`, portapapeles, prompts de
contraseña ni terminal scrollback sin revisar. Finaliza con la sección de
limpieza del [plan E2E manual](e2e-manual.md).
