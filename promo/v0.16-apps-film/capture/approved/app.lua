local app={api=1,types={'card'}}
local cols={'todo','doing','done'}
local names={todo='To do',doing='Doing',done='Done'}
local function priority(c) local p=c.priority; if p=='high' or p=='low' then return p end return 'normal' end
local function copy(c) local t={} for k,v in pairs(c) do t[k]=v end return t end
local function title(r) return type(r.content.title)=='string' and r.content.title or '(no title)' end
local function groups(s)
 local g={todo={},doing={},done={}}
 for _,r in ipairs(s.cards) do local c=r.content; if g[c.column] and (s.filter=='all' or priority(c)==s.filter) and title(r):lower():find(s.query:lower(),1,true) then table.insert(g[c.column],r) end end
 for _,v in pairs(g) do table.sort(v,function(a,b) local x,y=tonumber(a.content.order) or 0,tonumber(b.content.order) or 0; if x~=y then return x<y end return a.id<b.id end) end
 return g
end
local function locate(s) local g=groups(s); for ci,n in ipairs(cols) do for i,r in ipairs(g[n]) do if r.id==s.sel then return ci,i,g,r end end end return nil,nil,g end
local function keep(s) local ci,_,g=locate(s); if ci then return end s.sel=nil; for _,n in ipairs(cols) do if g[n][1] then s.sel=g[n][1].id; return end end end
local function put(s,r) for i,c in ipairs(s.cards) do if c.id==r.id then s.cards[i]=r; return end end table.insert(s.cards,r) end
local function remove(s,id) for i,r in ipairs(s.cards) do if r.id==id then table.remove(s.cards,i); return end end end
local function load(s) s.list_id=data.list{type='card',tag='load'}; s.touched={}; s.status='Loading...' end
function app.init(ctx) local s={cards={},query='',filter='all',status=''}; load(s); return s end
function app.reload(s,ctx) s.filter=s.filter or 'all'; s.query=s.query or ''; s.search_open=s.search_open or false; s.status=s.status or ''; keep(s); load(s) end
local function update(r,field,value) local c=copy(r.content); c[field]=value; data.update{record_id=r.id,rev=r.rev,content=c,tag={field=field,value=value}} end
function app.key(s,k,ctx)
 local n=k.name
 if s.form then
  if n=='esc' then s.form=nil elseif n=='enter' then local t=s.form.value:gsub('^%s+',''):gsub('%s+$',''); if t~='' and not s.form.saving then s.form.saving=true; data.create{type='card',content={title=t,column='todo',order=host.time(),priority='normal'},tag='create'} end else ui.edit(s.form,k) end return
 end
 if s.search_open then
  if n=='esc' then s.query=''; s.search_open=false elseif n=='enter' then s.search_open=false else local f={value=s.query,cursor=s.search_cursor}; ui.edit(f,k); s.query=f.value; s.search_cursor=f.cursor end keep(s); return
 end
 local ci,i,g,r=locate(s)
 if n=='q' then host.quit()
 elseif n=='n' then s.form={value=''}
 elseif n=='/' then s.search_open=true; s.search_cursor=nil
 elseif n=='esc' then s.query=''; s.filter='all'; keep(s)
 elseif n=='r' then load(s)
 elseif n=='f' then local nextf={all='high',high='normal',normal='low',low='all'}; s.filter=nextf[s.filter] or 'all'; keep(s)
 elseif n=='p' and r then local nextp={normal='high',high='low',low='normal'}; update(r,'priority',nextp[priority(r.content)])
 elseif ci then
  if n=='j' or n=='down' or n=='k' or n=='up' then local d=(n=='j' or n=='down') and 1 or -1; local list=g[cols[ci]]; s.sel=list[math.max(1,math.min(#list,i+d))].id
  elseif n=='h' or n=='left' or n=='l' or n=='right' then local d=(n=='l' or n=='right') and 1 or -1; local c=ci+d; while cols[c] do local list=g[cols[c]]; if #list>0 then s.sel=list[math.min(i,#list)].id; break end c=c+d end
  elseif n==']' or n=='[' then local c=cols[ci+(n==']' and 1 or -1)]; if c then update(r,'column',c) end end
 end
end
function app.paste(s,text,ctx) if s.form then ui.paste(s.form,text) elseif s.search_open then local f={value=s.query,cursor=s.search_cursor}; ui.paste(f,text); s.query=f.value; s.search_cursor=f.cursor; keep(s) end end
function app.reply(s,r,ctx)
 if r.tag=='load' then
  if r.id~=s.list_id then return end
  if r.ok then local out={}; for _,c in ipairs(r.records) do if not (s.touched or {})[c.id] then table.insert(out,c) end end for _,c in ipairs(s.cards) do if (s.touched or {})[c.id] then table.insert(out,c) end end s.cards=out; s.touched={}; s.status='Loaded.' else s.status='Load failed; r retry.' end
 elseif not r.ok then
  if r.error.code=='conflict' and r.current then local field,value; if type(r.tag)=='table' then field,value=r.tag.field,r.tag.value elseif r.tag=='move' or r.tag=='merge' then field,value='column',r.request.content.column end; if field then put(s,r.current); if s.touched then s.touched[r.current.id]=true end update(r.current,field,value); keep(s); return end end
  if s.form then s.form.saving=false end
  if r.error.code=='rate_limited' then s.status='Retry in '..r.error.retry_after..' s.' else s.status='Failed: '..r.error.code end
 else
  if r.record then put(s,r.record); if s.touched then s.touched[r.record.id]=true end end
  if r.tag=='create' then s.form=nil; s.sel=r.record.id end s.status='Saved.'
 end keep(s)
end
function app.push(s,p,ctx) if s.touched then s.touched[p.record.id]=true end if p.change=='delete' then remove(s,p.record.id) else put(s,p.record) end keep(s) end
function app.view(s,ctx)
 local g=groups(s); local wide=ctx.width>=60; local sections={}; local counts={#g.todo,#g.doing,#g.done}; local given={0,0,0}; local room=math.max(0,ctx.height-7)
 while room>0 do local changed=false; for i=1,3 do if room>0 and given[i]<counts[i] then given[i]=given[i]+1; room=room-1; changed=true end end if not changed then break end end
 for ci,n in ipairs(cols) do local items,sel={},nil; for i,r in ipairs(g[n]) do items[i]='['..priority(r.content):sub(1,1):upper()..'] '..title(r); if r.id==s.sel then sel=i end end sections[ci]=ui.column{size=not wide and given[ci]+1 or nil,ui.text{names[n]..' ('..#items..')',bold=true,fg='accent'},ui.list{items=items,selected=sel,id='col'..ci}} end
 local parts={ui.text{'Priority: '..s.filter..'  f filter / search',fg='accent'}}
 if wide then parts[#parts+1]=ui.row(sections) else for _,v in ipairs(sections) do parts[#parts+1]=v end parts[#parts+1]=ui.spacer{} end
 if s.search_open or s.query~='' then parts[#parts+1]=ui.input{value=s.query,cursor=s.search_cursor,focused=s.search_open,placeholder='Search titles'} end
 parts[#parts+1]=ui.text{'n new  p priority  [ ] move  Esc reset',fg='muted'}; parts[#parts+1]=ui.text{s.status,fg='muted'}
 local board=ui.column(parts); if not s.form then return board end return ui.layers{board,ui.popup{title='New card (normal priority)',width=math.min(38,ctx.width),height=4,ui.input{value=s.form.value,cursor=s.form.cursor,focused=true},ui.text{'Enter save  Esc cancel',fg='muted'}}}
end
return app