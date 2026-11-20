---@meta
-- app-api.d.lua: the contract between an app and bzz, `api = 1`, as
-- declarations for lua-language-server. Never run. The contract in words is
-- app-api.md; a test (apps::runtime::tests::contract) fails when the two
-- and the runtime name different things.
--
-- Language: Lua 5.4. Available besides what is declared here: the base
-- functions assert error getmetatable ipairs next pairs pcall rawequal
-- rawget rawlen rawset select setmetatable tonumber tostring type xpcall
-- _G _VERSION, and the libraries string (no string.dump), table, math, utf8.
-- Absent on purpose: io, os, package, require, load, loadfile, dofile,
-- debug, coroutine, collectgarbage. There is no file, network, process or
-- environment access of any kind.

----------------------------------------------------------------------------
-- The app: the script must end with `return app`.
----------------------------------------------------------------------------

---@class App
---The number of the contract. Required; bzz knows 1 and refuses another.
---@field api integer
---The record types the app may read and write (the fence). Required: 1 to
---16 names, each a lower-case letter and up to 31 of a-z, 0-9, _ and -.
---@field types string[]
---Required. Called after every round of events; returns the screen.
---@field view fun(state: table, ctx: Ctx): Element|string
---Called once at start. Returns the state table that every other handler
---receives. Default: an empty table.
---@field init? fun(ctx: Ctx): table
---A key press. Ctrl+C never arrives: bzz keeps it to leave the app.
---@field key? fun(state: table, key: Key, ctx: Ctx)
---A bracketed paste, as one string (it may contain line breaks).
---@field paste? fun(state: table, text: string, ctx: Ctx)
---The reply to a data.* request.
---@field reply? fun(state: table, reply: Reply, ctx: Ctx)
---A change by another participant, to a record of a declared type.
---@field push? fun(state: table, push: Push, ctx: Ctx)
---The app's region changed size (ctx has the new size).
---@field resize? fun(state: table, ctx: Ctx)
---A timer set with host.after fired.
---@field timer? fun(state: table, tag: any, ctx: Ctx)
---After a new version replaced the script. `state` is the previous state,
---copied into the new version (functions and metatables in it are dropped).
---Return a table to replace the state, or nothing to keep it.
---@field reload? fun(state: table, ctx: Ctx): table|nil

---Handlers change `state` in place; their return values are ignored, except
---those of init and reload. The top level of the script can only compute:
---data.list/create/update/delete, host.after and host.quit are for handlers.

---A participant. Compare and store `id`; show `name`. When bzz learns a name
---or a name changes, the records that person signed are pushed again as
---updates with the same rev.
---@class Who
---@field id string   the public key, 64 hex digits; it never changes
---@field name string for showing: the name bzz knows now, or a short form of the id

---@class Ctx
---@field width integer  columns of the app's region
---@field height integer rows of the app's region
---@field user Who       the local participant

---@class Key
---A printable key is the character itself ("j", "J", "/"); otherwise one of
---"space" "enter" "esc" "backspace" "tab" "up" "down" "left" "right" "home"
---"end" "pageup" "pagedown" "delete" "insert" "f1".."f12".
---Shift+Tab is name "tab" with shift = true.
---@field name string
---The text the key types, or nil (for a control key, and with ctrl or alt).
---@field text string|nil
---@field ctrl boolean
---@field alt boolean
---@field shift boolean

----------------------------------------------------------------------------
-- data: records shared through the relay. A request is sent at once; the
-- reply arrives later in app.reply, matched by its id and your tag. Replies
-- can arrive in any order.
----------------------------------------------------------------------------

---@class Record
---@field id string      set by the relay; ids do not sort by creation
---@field type string    the app's own short name of the type
---@field rev string     the revision: opaque; pass it back unchanged
---@field content table  the app's own fields (a JSON null arrives as nil); empty if another client wrote no table
---@field by Who         who signed this revision: the last writer

---@class DataError
---@field code "unavailable"|"conflict"|"not_found"|"bad_request"|"rate_limited"|"too_large"|"limit"|"refused"
---@field message string
---@field retry_after? integer rate_limited: the seconds to wait; until then no write is sent

---@class Reply
---@field id integer        the id data.* returned
---@field tag any           the tag given in the request (nil if none)
---@field op "list"|"create"|"update"|"delete"
---@field ok boolean
---@field records? Record[] list: the matching records, in no order
---@field record? Record    create, update: as stored; delete: its last state
---@field error? DataError  when ok is false
---@field current? Record   on a conflict: the record as it is now
---@field request table     the request as sent (id, op, type, match, record_id, rev, content)

---Pushes of a type begin with the app's first list of that type.
---@class Push
---@field change "create"|"update"|"delete"
---@field record Record     for delete: its last state
---@field by Who            who made the change; for a record simply gone after a resync, who signed it last

data = {}

---JSON null, for content you send: `{ decided_by = data.null }` is sent as
---`"decided_by": null`, where `decided_by = nil` would leave the field out.
---Only for writing: a null that arrives is always nil, never data.null.
---@type lightuserdata
data.null = nil

---Lists the records of a declared type, in no order, from what bzz holds:
---the reply comes at once, also without a connection. `match` keeps records
---whose title (content.title, or the app's and the type's names) contains
---it, ignoring case.
---@param req {type: string, match?: string, tag?: any}
---@return integer id
function data.list(req) end

---Creates a record. `content` must be a table with named fields. The apps
---of one person may write 30 times a minute; past it the reply is
---rate_limited with retry_after and nothing is sent.
---@param req {type: string, content: table, tag?: any}
---@return integer id
function data.create(req) end

---Replaces a record's whole content. `rev` must be the record's current rev,
---or the reply is a conflict that carries `current`. Only a record of a
---declared type that the app received in a reply or a push can be updated.
---@param req {record_id: string, rev: string, content: table, tag?: any}
---@return integer id
function data.update(req) end

---Deletes a record (same rules as update).
---@param req {record_id: string, rev: string, tag?: any}
---@return integer id
function data.delete(req) end

----------------------------------------------------------------------------
-- host
----------------------------------------------------------------------------

host = {}

---Closes the app after the current handler returns.
function host.quit() end

---Writes a line to the app's log.
---@param ... any
function host.log(...) end

---Calls app.timer(state, tag, ctx) once, after `ms` milliseconds (100 at
---least). The tag must be plain data.
---@param ms integer
---@param tag any
function host.after(ms, tag) end

---Seconds since 1970-01-01 UTC, with fractions.
---@return number
function host.time() end

---The name bzz knows for a participant's id, or a short form of the id
---while it knows none. bzz then looks it up and, when it learns it, calls
---app.view again. For showing a person stored by id in a field.
---@param id string at most 256 bytes
---@return string
function host.name(id) end

---The same as host.log: print never writes to the terminal.
---@param ... any
function print(...) end

----------------------------------------------------------------------------
-- ui: elements. Each constructor takes one table. Positional items [1],
-- [2], ... are children or content; named fields are properties. An unknown
-- property is an error at the calling line.
----------------------------------------------------------------------------

ui = {}

---A colour: a name of the theme (accent muted error warning success
---selection border title), a fixed name (black red green yellow blue
---magenta cyan gray dark_gray light_red light_green light_yellow light_blue
---light_magenta light_cyan white reset), "#rrggbb", or an index 0-255.
---@alias Color string|integer
---A size inside a row (width) or a column (height): cells (10), "50%",
---"1/3", "min:10", "max:10", "fill" or "fill:2" (a share of what is left).
---@alias Size integer|string

---The style fields every element, span and line accepts. A property that
---takes a Style also takes the name of a style of the theme ("selection").
---@class Style
---@field fg? Color
---@field bg? Color
---@field bold? boolean
---@field dim? boolean
---@field italic? boolean
---@field underline? boolean
---@field reversed? boolean
---@field crossed_out? boolean

---@class Element: Style
---@field size? Size  this element's size in its parent row or column

---A piece of text with its own style (inside text, line, items, cells).
---@alias Span table
---One line of spans (strings or ui.span).
---@alias LineLike string|Span|table

---@param t {[integer]: LineLike, align?: "left"|"center"|"right", wrap?: boolean, scroll?: integer}|Element|string
---@return Element
function ui.text(t) end

---@param t {[1]: string}|Style|string
---@return Span
function ui.span(t) end

---@param t {[integer]: string|Span, align?: "left"|"center"|"right"}|Style
---@return table
function ui.line(t) end

---@param t {[integer]: Element|string, title?: LineLike, title_bottom?: LineLike, title_align?: "left"|"center"|"right", borders?: string|string[], border_type?: "plain"|"rounded"|"double"|"thick", border_style?: Style|string, padding?: integer|integer[], spacing?: integer}|Element
---@return Element
function ui.block(t) end

---@param t {[integer]: Element|string, spacing?: integer, flex?: "legacy"|"start"|"center"|"end"|"space_between"|"space_around"|"space_evenly", margin?: integer}|Element
---@return Element
function ui.column(t) end

---@param t {[integer]: Element|string, spacing?: integer, flex?: "legacy"|"start"|"center"|"end"|"space_between"|"space_around"|"space_evenly", margin?: integer}|Element
---@return Element
function ui.row(t) end

---@param t {items: LineLike[], selected?: integer, highlight_symbol?: string, highlight?: Style|string, id?: string, direction?: "top_to_bottom"|"bottom_to_top", scroll_padding?: integer}|Element
---@return Element
function ui.list(t) end

---@param t {header?: LineLike[], rows: LineLike[][], footer?: LineLike[], widths?: Size[], selected?: integer, highlight?: Style|string, highlight_symbol?: string, column_spacing?: integer, header_style?: Style|string, id?: string}|Element
---@return Element
function ui.table(t) end

---@param t {titles: LineLike[], selected?: integer, divider?: string, highlight?: Style|string}|Element
---@return Element
function ui.tabs(t) end

---@param t {ratio?: number, percent?: number, label?: string, gauge_style?: Style|string, use_unicode?: boolean}|Element
---@return Element
function ui.gauge(t) end

---@param t {ratio?: number, percent?: number, label?: string, filled_style?: Style|string, unfilled_style?: Style|string}|Element
---@return Element
function ui.line_gauge(t) end

---@param t {data: number[], max?: integer, direction?: "left_to_right"|"right_to_left"}|Element
---@return Element
function ui.sparkline(t) end

---@param t {bars: table[], bar_width?: integer, bar_gap?: integer, max?: integer, horizontal?: boolean, value_style?: Style|string, label_style?: Style|string}|Element
---@return Element
function ui.bar_chart(t) end

---@param t {orientation?: "vertical_right"|"vertical_left"|"horizontal_bottom"|"horizontal_top", length: integer, position?: integer, viewport?: integer, thumb_style?: Style|string, track_style?: Style|string}|Element
---@return Element
function ui.scrollbar(t) end

---@param t {value?: string, cursor?: integer, placeholder?: string, focused?: boolean}|Element
---@return Element
function ui.input(t) end

---@param t {[integer]: Element|string}
---@return Element
function ui.layers(t) end

---@param t {[integer]: Element|string, width?: Size, height?: Size, title?: LineLike, border_type?: "plain"|"rounded"|"double"|"thick", spacing?: integer}|Element
---@return Element
function ui.popup(t) end

---@param t? {size?: Size}
---@return Element
function ui.spacer(t) end

---Applies a key to a one-line field table {value = string, cursor = integer}:
---typing inserts at the cursor; backspace, delete, left, right, home, end,
---Ctrl+U (delete to the start). Updates field.value and field.cursor.
---@param field {value?: string, cursor?: integer}
---@param key Key
---@return boolean handled  false for keys it does not use (enter, esc, up...)
function ui.edit(field, key) end

---Inserts pasted text into a field; each CR, LF or CRLF becomes one space.
---@param field {value?: string, cursor?: integer}
---@param text string
---@return boolean
function ui.paste(field, text) end

---Display width in terminal cells (East Asian wide characters count 2).
---@param s string
---@return integer
function ui.width(s) end

---Cuts s to at most `width` cells, ending in "…" when it was cut.
---@param s string
---@param width integer
---@return string
function ui.truncate(s, width) end
