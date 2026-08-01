# Plan manual de pruebas E2E

Este plan empieza por el recorrido mínimo y deja las pruebas destructivas de
recuperación para el final. Usa siempre una identidad de prueba; no uses una
clave administrativa para mensajes, reacciones o borrados.

Para operar este recorrido desde un panel automatizable, consulta la
[guía E2E asistida con Herdr](e2e-herdr.md).

## 0. Preparación aislada

```bash
cp .env.sample .env
$EDITOR .env
set -a; source .env; set +a
cargo build --release --locked
"$BZZ_BIN" --version
"$BZZ_BIN" paths
"$BZZ_BIN" check
```

Edita como mínimo `BZZ_RELAY_URL`. El relay debe ser Buzz-compatible y el
administrador debe añadir la pubkey de prueba como miembro. Usa el binario
`release`: las compilaciones debug tienen rutas y servicio de keychain
separados deliberadamente.

Resultado esperado:

- versión `0.1.0`;
- config, datos y caché debajo de `BZZ_E2E_ROOT`;
- `configuration, theme, and database are valid`;
- ningún secreto en `.env` ni en la salida.

## 1. Arranque vacío y restauración del terminal

```bash
"$BZZ_BIN"
```

Comprueba `Welcome to bzz`, abre/cierra ayuda con `?`/`Esc` y sal con `Q`.
El terminal debe recuperar cursor, eco y pantalla normal.

## 2. Identidad y backup (básico)

```bash
"$BZZ_BIN" identity new --label "$BZZ_IDENTITY_LABEL" \
  --backend "$BZZ_IDENTITY_BACKEND"
"$BZZ_BIN" identity list
"$BZZ_BIN" identity verify <IDENTITY_ID>
"$BZZ_BIN" identity backup <IDENTITY_ID> \
  --output "$BZZ_E2E_ROOT/identity.ncryptsec"
```

Rellena `BZZ_IDENTITY_ID` y `BZZ_PUBLIC_KEY` en `.env`, y vuelve a hacer
`source .env`. La creación y el backup solicitan contraseñas sin eco.

Resultado esperado:

- `identity list` solo muestra UUID, label, pubkey y backend;
- `identity verify` confirma exactamente la pubkey configurada;
- nunca aparece `nsec1…` ni clave hexadecimal privada;
- el backup empieza internamente por `ncryptsec1`, pero el comando solo imprime
  su ruta y pubkey;
- en Unix, `stat -c '%a' "$BZZ_E2E_ROOT/identity.ncryptsec"` devuelve `600`;
- repetir el backup sobre la misma ruta falla sin sobrescribirla.

Guarda temporalmente el backup y su contraseña: serán necesarios para la
prueba de recuperación.

## 3. Comunidad, autenticación y caché (básico)

Primero, el operador debe añadir `BZZ_PUBLIC_KEY` como miembro **normal** del
relay. En una instalación administrada, prefiere `buzz-admin add-member` dentro
del relay antes que desbloquear un owner key. Después configura bzz:

```bash
"$BZZ_BIN" community add \
  "$BZZ_COMMUNITY_LABEL" \
  "$BZZ_RELAY_URL" \
  "$BZZ_IDENTITY_ID"
"$BZZ_BIN" community list
"$BZZ_BIN" check
```

Rellena `BZZ_COMMUNITY_ID` con el UUID devuelto. La etiqueta local de identidad
no es un perfil público: si la interfaz administrativa busca miembros por
nombre, publica primero un perfil Nostr kind `0` de prueba o usa una ruta
administrativa que acepte la pubkey exacta. Crea un canal activo dedicado (por
ejemplo, `bzz-e2e-manual`) y añade la identidad de prueba. No reutilices un
canal de negocio ni uno archivado.

Abre `"$BZZ_BIN"`.

Resultado esperado:

1. `connecting`;
2. `authenticating`;
3. opcionalmente `backfilling`;
4. `online`;
5. canal E2E unido e historial visible.

No debe aparecer `access denied`, `clock skew` ni cambio de pubkey.

## 4. Mensaje y reinicio (básico)

1. Selecciona el canal E2E dedicado con `j/k` y `Enter`.
2. Pulsa `i`, escribe `E2E básico <fecha-hora>` y pulsa `Enter`.
3. Espera a que desaparezca `[pending]`.
4. Sal con `Q`, vuelve a abrir y localiza el mismo mensaje.

Si el relay responde `channel is archived`, conserva la evidencia del rechazo
pero cambia a un canal E2E activo; no publiques como alternativa en un canal de
producción cualquiera.

Resultado esperado: un único mensaje, ACK confirmado y recuperación desde la
caché tras reiniciar.

**Detente aquí en la primera sesión.** Si 0–4 pasan, el recorrido esencial está
validado.

## 5. Conversación

- Draft: `i`, escribe, `Esc`, vuelve a `i`; el texto reaparece.
- Thread: selecciona mensaje, `Enter`/`Ctrl-]`, responde y cierra con `Esc`.
- Reacción: `r`, selecciona, `Enter`; repite para retirar la misma reacción.
- Borrado propio: selecciona tu mensaje, `D`, confirma con `y`.
- No leído: `U`, comprueba el indicador, abre otro canal, vuelve al canal E2E
  con el finder, pulsa `Enter` y `G`; el indicador debe desaparecer incluso si
  el marcador remoto ya estaba adelantado.
- Finder: `Ctrl-p`; con consulta vacía debe priorizar `#` unidos. Busca además
  un canal `+` abierto, ábrelo sin publicar y vuelve al canal E2E.

Verifica que threads, reacciones y borrados no se duplican y que nunca puedes
borrar mensajes ajenos.

### Apariencia

1. Pulsa `Ctrl-y`, filtra `nord` y mueve la selección: la vista cambia sin
   modificar mensajes ni unread.
2. Pulsa `Esc`: debe volver exactamente el tema anterior.
3. Reabre, selecciona un tema y pulsa `Enter`; reinicia y comprueba que persiste.
4. Usa `Tab` en el picker para probar los alcances global y comunidad.
5. Crea un `theme.toml` con un color inválido junto a otro válido y ejecuta
   `bzz theme check`: solo la hoja inválida debe producir warning.
6. Rompe temporalmente la sintaxis TOML: `bzz theme check` debe fallar, pero la
   TUI debe arrancar con el preset compilado y permitir recuperar el archivo.
7. Restaura o elimina el override y ejecuta `bzz theme reset`.

## 6. Backup portable sin destruir la identidad activa

Importa el backup como una segunda entrada:

```bash
"$BZZ_BIN" identity import-backup \
  --label e2e-restored \
  --input "$BZZ_E2E_ROOT/identity.ncryptsec" \
  --backend "$BZZ_IDENTITY_BACKEND"
"$BZZ_BIN" identity list
"$BZZ_BIN" identity verify <RESTORED_ID>
```

Resultado esperado: ambos UUID son distintos, pero ambas pubkeys son
idénticas. Una contraseña incorrecta debe devolver un error genérico sin
modificar la configuración. Después elimina la entrada duplicada:

```bash
"$BZZ_BIN" identity remove <RESTORED_ID> --yes
```

También puedes comprobar una restauración in-place segura:

```bash
"$BZZ_BIN" identity restore-backup "$BZZ_IDENTITY_ID" \
  --input "$BZZ_E2E_ROOT/identity.ncryptsec"
"$BZZ_BIN" identity verify "$BZZ_IDENTITY_ID"
```

Una copia perteneciente a otra pubkey debe rechazarse.

## 7. Recuperación de keychain y modo caché (avanzado/destructivo)

Hazlo únicamente después de verificar el backup NIP-49.

1. Cierra bzz.
2. Desde el gestor de credenciales del sistema, elimina solo la entrada:
   - servicio release: `dev.arpagon.bzz`;
   - cuenta: `identity:<BZZ_IDENTITY_ID>`.
3. Abre bzz.

Resultado esperado:

- estado `identity missing`;
- historial y canales cacheados visibles;
- no se abre conexión al relay;
- `i`, `r` y `D` no publican nada;
- el estado explica cómo restaurar.

Restaura sin cambiar UUID, comunidad ni pubkey:

```bash
"$BZZ_BIN" identity restore-backup "$BZZ_IDENTITY_ID" \
  --input "$BZZ_E2E_ROOT/identity.ncryptsec"
"$BZZ_BIN" identity verify "$BZZ_IDENTITY_ID"
"$BZZ_BIN"
```

Debe volver a `online`. Para probar `identity locked`, bloquea temporalmente el
keychain/Secret Service y relanza; bzz debe mostrar caché en solo lectura, nunca
generar una identidad nueva. Desbloquea el keychain y relanza para recuperar.

## 8. Red, múltiples clientes y comunidades (avanzado)

- Para no cortar la red del host, copia config/base de datos a un directorio
  desechable y cambia solo esa copia a un puerto loopback cerrado. Comprueba
  historial en `offline cache`, ejecuta `:reconnect` y vuelve después al relay
  real.
- Para multicliente real usa bases SQLite distintas. El segundo cliente debe
  generar otro `client_id`; publica en ambas direcciones y comprueba una sola
  copia, dos slots y el mismo `read_at` máximo.
- Deja un hueco mayor que una página, reconecta y verifica backfill completo.
  Hazlo en el harness aislado, no generando cientos de mensajes en producción.
- Configura una segunda comunidad de prueba y cambia con `1`/`2`; comprueba
  aislamiento. Si solo hay un relay disponible, cubre este punto con el test
  automatizado multicomunidad en vez de duplicar el mismo host.

## Registro de resultados

| Bloque | Resultado | Evidencia/notas |
|---|---|---|
| 0. Preparación | ☐ | |
| 1. Arranque vacío | ☐ | |
| 2. Identidad/backup | ☐ | |
| 3. Comunidad/auth | ☐ | |
| 4. Mensaje/reinicio | ☐ | |
| 5. Conversación | ☐ | |
| 6. Backup portable | ☐ | |
| 7. Recuperación | ☐ | |
| 8. Red/multicliente | ☐ | |

## Limpieza

```bash
"$BZZ_BIN" community remove "$BZZ_COMMUNITY_ID" --purge --yes
"$BZZ_BIN" identity remove "$BZZ_IDENTITY_ID" --yes
rm -rf "$BZZ_E2E_ROOT"
```

Borra también cualquier backup copiado fuera de `BZZ_E2E_ROOT` que ya no
necesites. Nunca incluyas `.env`, `nsec`, backups o contraseñas en evidencias.
