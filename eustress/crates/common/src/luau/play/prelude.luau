--!nocheck
-- Eustress Play prelude. Runs once in every Play VM, after the Rust host
-- has installed `__host` (DataModel access), the value types and `game`.
-- Evaluates to the Host table the Rust side drives each frame.
--
-- Nothing here writes a global: the VM is sandboxed, so Rust installs every
-- global this chunk returns (see `play::install_prelude`).

local host = __host
local create, resume, yield, running, status, close, isyieldable =
	coroutine.create, coroutine.resume, coroutine.yield, coroutine.running,
	coroutine.status, coroutine.close, coroutine.isyieldable
local pack = table.pack
local unpack = table.unpack
local tinsert = table.insert
local clock = os.clock

local Host = {}

-- ─────────────────────────────────────────────────────────────────────────
-- Error reporting
-- ─────────────────────────────────────────────────────────────────────────

-- Which script each thread and connection belongs to (see ownership below),
-- declared here for the reporters.
local ownerOf = setmetatable({}, { __mode = "k" })
local threadsOf = {}
local connsOf = {}
-- Each running script's path, for the Output lines about it.
local nameOf = {}
-- Each script's top-level thread, with the script it runs.
local bodies = setmetatable({}, { __mode = "k" })

local PRELUDE_CHUNK = "EustressPlayPrelude"

-- Where `thread` is, innermost call first, in script code only: its frames
-- ("ServerScriptService.GameDirector:12 function spawnWave"), and the chunk
-- and line of the innermost one.
local function stackOf(thread)
	local frames, chunk, line = {}, nil, 0
	for level = 0, 64 do
		local src, at, name = debug.info(thread, level, "sln")
		if src == nil then
			break
		end
		if src ~= PRELUDE_CHUNK and src ~= "[C]" and at ~= nil and at > 0 then
			if chunk == nil then
				chunk, line = src, at
			end
			local frame = src .. ":" .. at
			if name ~= nil and name ~= "" then
				frame ..= " function " .. name
			end
			tinsert(frames, frame)
		end
	end
	return frames, chunk, line
end

local function report(err, thread)
	local frames, chunk, line = stackOf(thread)
	-- Errors from the host carry mlua's "runtime error: " in front and its
	-- own traceback behind, which Roblox's never show (the frames go on the
	-- line instead); a message with no position gets the innermost script
	-- frame's.
	local msg = string.gsub(tostring(err), "^runtime error: ", "")
	local traceback = string.find(msg, "\nstack traceback:", 1, true)
	if traceback ~= nil then
		msg = string.sub(msg, 1, traceback - 1)
	end
	msg = string.gsub(msg, "%s+$", "")
	if chunk ~= nil and not string.find(msg, "^[^\n]-:%d+: ") then
		msg = chunk .. ":" .. line .. ": " .. msg
	end
	-- A timeout names where the script was; the thread itself stops a call
	-- or two later.
	local stoppedIn, stoppedAt = string.match(msg, "^Script timeout at ([^\n]-):(%d+): ")
	if stoppedIn ~= nil then
		chunk, line = stoppedIn, tonumber(stoppedAt)
	end
	host.output("error", msg, nameOf[ownerOf[thread]], chunk, line, frames)
end
Host.report = report

-- A thread that will never resume, or may not: warn, where it waits.
local function warnStuck(thread, text)
	local frames, chunk, line = stackOf(thread)
	host.output("warn", text, nameOf[ownerOf[thread]], chunk, line, frames)
end

-- ─────────────────────────────────────────────────────────────────────────
-- Scheduler
-- ─────────────────────────────────────────────────────────────────────────

local Sched = {
	now = 0,
	sleeping = {}, -- { co, at, started, args, cancelled }
	deferred = {}, -- { co, args, cancelled }
}
Host.Sched = Sched

-- Threads parked on the scheduler (task.wait, Signal:Wait, a handler runner
-- back in its pool), each resumed when its time or signal comes.
local parked = setmetatable({}, { __mode = "k" })
-- Threads the scheduler resumed that then yielded with coroutine.yield,
-- parked on nothing, by when: only code holding the thread can resume it,
-- so one left alone too long is reported.
local rawYields = setmetatable({}, { __mode = "k" })

-- Every scheduler resume of a script thread. One from the host, rather than
-- from inside a running script, gets a fresh run budget: the watchdog stops
-- a script that runs too long without yielding.
local function resumeThread(co, ...)
	if running() == nil then
		host.newBudget()
	end
	parked[co] = nil
	rawYields[co] = nil
	local ok, err = resume(co, ...)
	if ok and not parked[co] and status(co) == "suspended" then
		rawYields[co] = Sched.now
	end
	return ok, err
end

-- A script's top-level code is done: say so, and what it left running.
local function finished(owner, ok)
	local conns, threads = 0, 0
	for c in pairs(connsOf[owner] or {}) do
		if c.Connected then
			conns += 1
		end
	end
	for co in pairs(threadsOf[owner] or {}) do
		if status(co) ~= "dead" then
			threads += 1
		end
	end
	local left = {}
	if conns > 0 then
		tinsert(left, conns .. (if conns == 1 then " connection" else " connections"))
	end
	if threads > 0 then
		tinsert(left, threads .. (if threads == 1 then " thread" else " threads"))
	end
	local summary = if #left > 0 then table.concat(left, " and ") .. " still running" else "nothing left running"
	local what = if ok then "Finished its top-level code; " else "Stopped by the error above; "
	host.lifecycle(what .. summary, nameOf[owner], owner)
end

local function safe_resume(co, ...)
	if co == nil or status(co) == "dead" then
		return
	end
	local ok, err = resumeThread(co, ...)
	if not ok then
		report(err, co)
	end
	local owner = bodies[co]
	if owner ~= nil and status(co) == "dead" then
		bodies[co] = nil
		finished(owner, ok)
	end
end

-- Handlers run on a reused runner thread. A handler that finishes without
-- yielding hands the runner back, so a signal firing every frame does not
-- create a coroutine per call; one that yields keeps it, and the next call
-- makes a fresh runner.
local freeRunner = nil
local function acquireAndCall(fn, ...)
	local acquired = freeRunner
	freeRunner = nil
	fn(...)
	freeRunner = acquired
end
local function runnerLoop(...)
	acquireAndCall(...)
	while true do
		parked[running()] = true
		acquireAndCall(yield())
	end
end
local function callHandler(fn, ...)
	if freeRunner == nil then
		freeRunner = create(runnerLoop)
		local ok, err = resumeThread(freeRunner, fn, ...)
		if not ok then
			report(err, freeRunner)
			freeRunner = nil
		end
		return
	end
	local runner = freeRunner
	local ok, err = resumeThread(runner, fn, ...)
	if not ok then
		report(err, runner)
		if freeRunner == runner then
			freeRunner = nil
		end
	end
end
Host.callHandler = callHandler

-- Which script each thread and connection belongs to, so destroying a script
-- stops what it started, as in Roblox: its threads end and its connections
-- drop, wherever they were made. A thread takes the owner of the thread that
-- made it. A handler runner is shared across scripts, so it carries a
-- handler's owner only while that handler runs, and goes back to the pool
-- untagged; a pooled runner left tagged would be killed with the first script
-- to stop, and silently eat the next handler call.
-- The tables themselves are declared above, with the reporters.

local function currentOwner()
	local co = running()
	return co and ownerOf[co]
end

-- Scripts written for Roblox, which work in studs (the importer marks them;
-- the host says so as each starts). A Humanoid's movement values convert for
-- them at the boundary, and so do the speeds Humanoid signals hand them; the
-- engine is metres. A tween converts by the script that made it.
local studsOwners = {}
local studsOverride = nil
local STUD_METRES = host.studMetres

function Host.ownerUsesStuds()
	if studsOverride ~= nil then
		return studsOverride
	end
	local o = currentOwner()
	return o ~= nil and studsOwners[o] == true
end

-- A speed in metres a second, in studs a second.
local function speedInStuds(speed, ...)
	if type(speed) == "number" then
		return speed / STUD_METRES, ...
	end
	return speed, ...
end

local function claim(co, owner)
	if owner == nil then
		return
	end
	ownerOf[co] = owner
	local set = threadsOf[owner]
	if set == nil then
		set = setmetatable({}, { __mode = "k" })
		threadsOf[owner] = set
	end
	set[co] = true
end

local function unclaim(co, owner)
	ownerOf[co] = nil
	local set = threadsOf[owner]
	if set then
		set[co] = nil
	end
end

-- One handler call on the current runner, under its connection's owner.
local function ownedCall(owner, fn, ...)
	local co = running()
	claim(co, owner)
	fn(...)
	unclaim(co, owner)
end

-- A script's body, started as a thread the script owns.
function Host.startScript(owner, fn, inStuds, name)
	if inStuds then
		studsOwners[owner] = true
	end
	nameOf[owner] = name
	local co = create(fn)
	claim(co, owner)
	bodies[co] = owner
	safe_resume(co)
	return co
end

-- A destroyed script stops: every thread it owns ends and every connection it
-- made drops. Called for each running script the DataModel destroys.
function Host.stopScript(owner)
	studsOwners[owner] = nil
	nameOf[owner] = nil
	local conns = connsOf[owner]
	connsOf[owner] = nil
	if conns then
		for c in pairs(conns) do
			c:Disconnect()
		end
	end
	local threads = threadsOf[owner]
	threadsOf[owner] = nil
	if threads then
		for co in pairs(threads) do
			ownerOf[co] = nil
			Sched.cancel(co)
		end
	end
end

-- `coroutine` as scripts see it: the real library, except that a coroutine a
-- script makes is that script's, so a `coroutine.wrap` loop ends with it too.
Host.ownedCoroutine = setmetatable({
	-- A thread a script resumes itself waits on nothing any more.
	resume = function(co, ...)
		if type(co) == "thread" then
			parked[co] = nil
			rawYields[co] = nil
		end
		return resume(co, ...)
	end,
	create = function(fn)
		local co = create(fn)
		claim(co, currentOwner())
		return co
	end,
	wrap = function(fn)
		local co = create(fn)
		claim(co, currentOwner())
		return function(...)
			local r = pack(resume(co, ...))
			if not r[1] then
				error(r[2], 0)
			end
			return unpack(r, 2, r.n)
		end
	end,
}, { __index = coroutine })

function Sched.spawn(fn, ...)
	local co = (type(fn) == "thread") and fn or create(fn)
	if ownerOf[co] == nil then
		claim(co, currentOwner())
	end
	safe_resume(co, ...)
	return co
end

function Sched.defer(fn, ...)
	local co = (type(fn) == "thread") and fn or create(fn)
	if ownerOf[co] == nil then
		claim(co, currentOwner())
	end
	tinsert(Sched.deferred, { co = co, args = pack(...) })
	return co
end

function Sched.delay(sec, fn, ...)
	local co = (type(fn) == "thread") and fn or create(fn)
	if ownerOf[co] == nil then
		claim(co, currentOwner())
	end
	tinsert(Sched.sleeping, { co = co, at = Sched.now + (tonumber(sec) or 0), started = Sched.now, args = pack(...) })
	return co
end

function Sched.wait(sec)
	local co = running()
	if co == nil or not isyieldable() then
		host.output("warn", "task.wait() called outside a script thread; it returned immediately")
		return 0
	end
	tinsert(Sched.sleeping, { co = co, at = Sched.now + math.max(tonumber(sec) or 0, 0), started = Sched.now })
	parked[co] = true
	return yield()
end

function Sched.cancel(co)
	if type(co) ~= "thread" then
		return
	end
	for _, e in ipairs(Sched.sleeping) do
		if e.co == co then
			e.cancelled = true
		end
	end
	for _, e in ipairs(Sched.deferred) do
		if e.co == co then
			e.cancelled = true
		end
	end
	if co ~= running() and status(co) ~= "dead" then
		pcall(close, co)
	end
end

-- Host entry: wake sleepers due at `now`, then run deferred threads.
function Sched.step(now)
	Sched.now = now
	if #Sched.sleeping > 0 then
		local due, keep = {}, {}
		for _, e in ipairs(Sched.sleeping) do
			if e.cancelled then
				-- dropped
			elseif e.at <= now then
				tinsert(due, e)
			else
				tinsert(keep, e)
			end
		end
		Sched.sleeping = keep
		table.sort(due, function(a, b)
			return a.at < b.at
		end)
		for _, e in ipairs(due) do
			if e.args then
				safe_resume(e.co, unpack(e.args, 1, e.args.n))
			else
				safe_resume(e.co, now - e.started)
			end
		end
	end
	Host.flushDeferred()
end

function Host.flushDeferred()
	local guard = 0
	while #Sched.deferred > 0 and guard < 16 do
		local d = Sched.deferred
		Sched.deferred = {}
		for _, e in ipairs(d) do
			if not e.cancelled then
				safe_resume(e.co, unpack(e.args, 1, e.args.n))
			end
		end
		guard += 1
	end
end

-- ─────────────────────────────────────────────────────────────────────────
-- Signals
-- ─────────────────────────────────────────────────────────────────────────

local Connection = {}
Connection.__index = Connection

function Connection:Disconnect()
	if not self.Connected then
		return
	end
	self.Connected = false
	local sig = self._signal
	local list = sig._handlers
	for i = #list, 1, -1 do
		if list[i] == self then
			table.remove(list, i)
			break
		end
	end
	if sig._onEmpty and #list == 0 and #sig._waiters == 0 then
		sig._onEmpty()
	end
end
Connection.disconnect = Connection.Disconnect

local Signal = {}
Signal.__index = Signal
Signal.__type = "RBXScriptSignal"

local function newSignal(onFirstConnect, onEmpty)
	return setmetatable({ _handlers = {}, _waiters = {}, _onFirst = onFirstConnect, _onEmpty = onEmpty }, Signal)
end
Host.newSignal = newSignal

function Signal:Connect(fn)
	assert(type(fn) == "function", "Connect expects a function")
	local c = setmetatable({ Connected = true, _fn = fn, _signal = self }, Connection)
	tinsert(self._handlers, c)
	local owner = currentOwner()
	if owner ~= nil then
		c._owner = owner
		local set = connsOf[owner]
		if set == nil then
			set = setmetatable({}, { __mode = "k" })
			connsOf[owner] = set
		end
		set[c] = true
	end
	if self._onFirst and #self._handlers == 1 then
		self._onFirst()
	end
	return c
end
Signal.connect = Signal.Connect

function Signal:Once(fn)
	local c
	c = self:Connect(function(...)
		c:Disconnect()
		fn(...)
	end)
	return c
end

function Signal:Wait()
	local co = running()
	if co == nil or not isyieldable() then
		error("Signal:Wait() must be called from a script thread", 2)
	end
	tinsert(self._waiters, co)
	if self._onFirst and #self._handlers == 0 and #self._waiters == 1 then
		self._onFirst()
	end
	parked[co] = true
	return yield()
end
Signal.wait = Signal.Wait

function Signal:Fire(...)
	local handlers = self._handlers
	if #handlers > 0 then
		-- Snapshot: a handler may disconnect or connect while we iterate.
		local snapshot = table.clone(handlers)
		for _, c in ipairs(snapshot) do
			if c.Connected then
				if c._owner ~= nil then
					if self._speed and studsOwners[c._owner] then
						callHandler(ownedCall, c._owner, c._fn, speedInStuds(...))
					else
						callHandler(ownedCall, c._owner, c._fn, ...)
					end
				else
					callHandler(c._fn, ...)
				end
			end
		end
	end
	local waiters = self._waiters
	if #waiters > 0 then
		self._waiters = {}
		for _, co in ipairs(waiters) do
			if self._speed and studsOwners[ownerOf[co]] then
				safe_resume(co, speedInStuds(...))
			else
				safe_resume(co, ...)
			end
		end
	end
end

function Signal:DisconnectAll()
	for _, c in ipairs(self._handlers) do
		c.Connected = false
	end
	self._handlers = {}
end

function Signal:HasListeners()
	return #self._handlers > 0 or #self._waiters > 0
end

-- Per-instance signals, created on first access. Keyed by instance id so a
-- destroyed instance's signals can be dropped in one step.
local instanceSignals = {}
Host.instanceSignals = instanceSignals

function Host.getSignal(id, name)
	local byName = instanceSignals[id]
	if byName == nil then
		byName = {}
		instanceSignals[id] = byName
	end
	local sig = byName[name]
	if sig == nil then
		sig = newSignal()
		-- A Humanoid's Running and Climbing hand their handlers a speed.
		sig._speed = name == "Running" or name == "Climbing"
		byName[name] = sig
		host.listen(id, name)
	end
	return sig
end

function Host.fireSignal(id, name, ...)
	local byName = instanceSignals[id]
	if byName == nil then
		return
	end
	local sig = byName[name]
	if sig ~= nil then
		sig:Fire(...)
	end
end

-- Threads left waiting on a destroyed instance's signals. They never resume
-- (as in Roblox); each one still suspended once the scripts it belonged to
-- have stopped is reported.
local orphans = {}

function Host.dropSignals(id, destroyed, instName)
	local byName = instanceSignals[id]
	if byName then
		for name, sig in pairs(byName) do
			if destroyed then
				for _, co in ipairs(sig._waiters) do
					tinsert(orphans, { co = co, what = "the " .. name .. " event of " .. (instName or "an instance") })
				end
			end
			sig:DisconnectAll()
		end
		instanceSignals[id] = nil
	end
end

-- Property-changed signals: GetPropertyChangedSignal(prop) and Changed.
local propSignals = {}
function Host.getPropertySignal(id, prop)
	local byProp = propSignals[id]
	if byProp == nil then
		byProp = {}
		propSignals[id] = byProp
	end
	local sig = byProp[prop]
	if sig == nil then
		sig = newSignal()
		byProp[prop] = sig
		host.watch(id)
	end
	return sig
end

function Host.firePropertyChanged(id, prop)
	local byProp = propSignals[id]
	if byProp then
		local sig = byProp[prop]
		if sig then
			sig:Fire()
		end
	end
	Host.fireSignal(id, "Changed", prop)
end

local attrSignals = {}
function Host.getAttributeSignal(id, name)
	local byName = attrSignals[id]
	if byName == nil then
		byName = {}
		attrSignals[id] = byName
	end
	local sig = byName[name]
	if sig == nil then
		sig = newSignal()
		byName[name] = sig
		host.listen(id, "AttributeChanged")
	end
	return sig
end

function Host.fireAttributeChanged(id, name)
	local byName = attrSignals[id]
	if byName and byName[name] then
		byName[name]:Fire()
	end
	Host.fireSignal(id, "AttributeChanged", name)
end

-- ─────────────────────────────────────────────────────────────────────────
-- task library and legacy globals
-- ─────────────────────────────────────────────────────────────────────────

Host.task = {
	wait = Sched.wait,
	spawn = Sched.spawn,
	defer = Sched.defer,
	delay = Sched.delay,
	cancel = Sched.cancel,
	synchronize = function() end,
	desynchronize = function() end,
}

function Host.wait(sec)
	local elapsed = Sched.wait(sec or 0.03)
	return elapsed, Sched.now
end

function Host.spawn(fn)
	return Sched.defer(fn)
end

function Host.delay(sec, fn)
	return Sched.delay(sec, fn)
end

-- ─────────────────────────────────────────────────────────────────────────
-- Enum
-- ─────────────────────────────────────────────────────────────────────────

-- Items are interned userdata made by the host, so `a == b` and table keys
-- both work.
local enumTypes = {}
local EnumMeta = {}
EnumMeta.__index = function(_, typeName)
	local t = enumTypes[typeName]
	if t == nil then
		local items = {}
		t = setmetatable({}, {
			__index = function(_, itemName)
				if itemName == "GetEnumItems" then
					return function()
						local out = {}
						for _, item in pairs(items) do
							tinsert(out, item)
						end
						return out
					end
				end
				local item = items[itemName]
				if item == nil then
					item = host.enumItem(typeName, itemName)
					items[itemName] = item
				end
				return item
			end,
			__tostring = function()
				return typeName
			end,
		})
		enumTypes[typeName] = t
	end
	return t
end
Host.Enum = setmetatable({}, EnumMeta)

-- ─────────────────────────────────────────────────────────────────────────
-- Services implemented in Lua
-- ─────────────────────────────────────────────────────────────────────────

-- TweenService: tweens any interpolable property on the Heartbeat.
local EasingFns = {
	Linear = function(t)
		return t
	end,
	Sine = function(t)
		return 1 - math.cos(t * math.pi / 2)
	end,
	Quad = function(t)
		return t * t
	end,
	Cubic = function(t)
		return t * t * t
	end,
	Quart = function(t)
		return t * t * t * t
	end,
	Quint = function(t)
		return t * t * t * t * t
	end,
	Exponential = function(t)
		return t == 0 and 0 or 2 ^ (10 * t - 10)
	end,
	Circular = function(t)
		return 1 - math.sqrt(1 - t * t)
	end,
	Back = function(t)
		local c1 = 1.70158
		return (c1 + 1) * t * t * t - c1 * t * t
	end,
	Elastic = function(t)
		if t == 0 or t == 1 then
			return t
		end
		local c4 = (2 * math.pi) / 3
		return -(2 ^ (10 * t - 10)) * math.sin((t * 10 - 10.75) * c4)
	end,
	Bounce = function(t)
		local function out(x)
			local n1, d1 = 7.5625, 2.75
			if x < 1 / d1 then
				return n1 * x * x
			elseif x < 2 / d1 then
				x -= 1.5 / d1
				return n1 * x * x + 0.75
			elseif x < 2.5 / d1 then
				x -= 2.25 / d1
				return n1 * x * x + 0.9375
			else
				x -= 2.625 / d1
				return n1 * x * x + 0.984375
			end
		end
		return 1 - out(1 - t)
	end,
}

local function enumName(v, default)
	if v == nil then
		return default
	end
	if type(v) == "string" then
		return (string.match(v, "([%w_]+)$")) or default
	end
	local ok, n = pcall(function()
		return v.Name
	end)
	if ok and n then
		return n
	end
	return default
end

local function ease(style, direction, t)
	local f = EasingFns[style] or EasingFns.Quad
	if direction == "In" then
		return f(t)
	elseif direction == "Out" then
		return 1 - f(1 - t)
	else
		if t < 0.5 then
			return f(t * 2) / 2
		end
		return 1 - f((1 - t) * 2) / 2
	end
end

local function lerpValue(a, b, alpha)
	local ta = typeof(a)
	if ta == "number" then
		return a + (b - a) * alpha
	elseif ta == "boolean" or ta == "string" or ta == "EnumItem" then
		return alpha >= 1 and b or a
	end
	return a:Lerp(b, alpha)
end

local activeTweens = {}
local Tween = {}
Tween.__index = Tween

function Tween:Play()
	if self.PlaybackState == "Playing" then
		return
	end
	self.PlaybackState = "Playing"
	self._elapsed = self._elapsed or 0
	self._from = {}
	studsOverride = self._studs
	for prop in pairs(self._goals) do
		self._from[prop] = self.Instance[prop]
	end
	studsOverride = nil
	activeTweens[self] = true
end

function Tween:Pause()
	if self.PlaybackState == "Playing" then
		self.PlaybackState = "Paused"
		activeTweens[self] = nil
	end
end

function Tween:Cancel()
	self.PlaybackState = "Cancelled"
	self._elapsed = 0
	activeTweens[self] = nil
	self.Completed:Fire("Cancelled")
end

function Tween:Destroy()
	activeTweens[self] = nil
end

local function stepTweens(dt)
	for tw in pairs(activeTweens) do
		tw._elapsed += dt
		local info = tw.TweenInfo
		local duration = math.max(info.Time, 1e-6)
		local t = (tw._elapsed - info.DelayTime) / duration
		if t >= 0 then
			local finished = t >= 1
			local ct = math.clamp(t, 0, 1)
			local alpha = ease(info._style, info._direction, ct)
			local inst = tw.Instance
			local alive = host.alive(inst)
			if alive then
				studsOverride = tw._studs
				for prop, goal in pairs(tw._goals) do
					local ok, err = pcall(function()
						inst[prop] = lerpValue(tw._from[prop], goal, alpha)
					end)
					if not ok then
						host.output("warn", "Tween: " .. tostring(err))
					end
				end
				studsOverride = nil
			end
			if finished or not alive then
				activeTweens[tw] = nil
				tw.PlaybackState = "Completed"
				tw.Completed:Fire("Completed")
			end
		end
	end
end
Host.stepTweens = stepTweens

local TweenInfoMeta = { __type = "TweenInfo" }
TweenInfoMeta.__index = TweenInfoMeta
Host.TweenInfo = {
	new = function(time, style, direction, repeatCount, reverses, delayTime)
		return setmetatable({
			Time = time or 1,
			EasingStyle = style or Host.Enum.EasingStyle.Quad,
			EasingDirection = direction or Host.Enum.EasingDirection.Out,
			RepeatCount = repeatCount or 0,
			Reverses = reverses or false,
			DelayTime = delayTime or 0,
		}, TweenInfoMeta)
	end,
}

Host.TweenService = {
	Create = function(_, instance, tweenInfo, goals)
		local info = {
			Time = tweenInfo and tweenInfo.Time or 1,
			DelayTime = tweenInfo and tweenInfo.DelayTime or 0,
			_style = enumName(tweenInfo and tweenInfo.EasingStyle, "Quad"),
			_direction = enumName(tweenInfo and tweenInfo.EasingDirection, "Out"),
		}
		return setmetatable({
			Instance = instance,
			TweenInfo = info,
			PlaybackState = "Begin",
			Completed = newSignal(),
			_goals = goals,
			_elapsed = 0,
			-- The units of the script that made it, for every step.
			_studs = Host.ownerUsesStuds(),
		}, Tween)
	end,
	GetValue = function(_, alpha, style, direction)
		return ease(enumName(style, "Quad"), enumName(direction, "Out"), math.clamp(alpha, 0, 1))
	end,
}

-- Debris: destroy after a delay.
Host.Debris = {
	AddItem = function(_, instance, lifetime)
		Sched.delay(lifetime or 10, function()
			if host.alive(instance) then
				instance:Destroy()
			end
		end)
	end,
}

-- WaitForChild yields until the child appears (or the timeout passes).
function Host.waitForChild(parent, name, timeout)
	local found = parent:FindFirstChild(name)
	if found then
		return found
	end
	local co = running()
	if co == nil or not isyieldable() then
		error("WaitForChild(" .. tostring(name) .. ") must be called from a script thread", 3)
	end
	local deadline = timeout and (Sched.now + timeout) or nil
	local warned = false
	local started = Sched.now
	while true do
		Sched.wait(0)
		found = parent:FindFirstChild(name)
		if found then
			return found
		end
		if deadline and Sched.now >= deadline then
			return nil
		end
		if not warned and not timeout and Sched.now - started > 5 then
			warned = true
			warnStuck(co, "Infinite yield possible on '" .. parent:GetFullName() .. ":WaitForChild(\"" .. tostring(name) .. "\")'")
		end
	end
end

-- ModuleScript cache for require().
local moduleCache = {}
local loading = {}
function Host.require(module)
	local cached = moduleCache[module]
	if cached ~= nil then
		return cached.value
	end
	if loading[module] then
		error("Requested module was required recursively: " .. module:GetFullName(), 2)
	end
	loading[module] = true
	local fn = host.compileModule(module)
	local ok, result = pcall(fn)
	loading[module] = nil
	if not ok then
		error("Requested module experienced an error while loading: " .. tostring(result), 2)
	end
	moduleCache[module] = { value = result }
	return result
end

-- ─────────────────────────────────────────────────────────────────────────
-- Callbacks, bindables and remotes (single-process Play: remotes loop back)
-- ─────────────────────────────────────────────────────────────────────────

local callbacks = {} -- [id][name] = function (OnInvoke, OnServerInvoke, ...)
function Host.setCallback(id, name, fn)
	local t = callbacks[id]
	if t == nil then
		t = {}
		callbacks[id] = t
	end
	t[name] = fn
	if name == "ProcessReceipt" then
		-- Purchases still waiting from earlier sessions load now.
		host.commerceWanted()
	end
end

local function callback(id, name)
	local t = callbacks[id]
	return t and t[name]
end

-- Events fire on the next resumption point, like Roblox's deferred mode, so
-- a handler that fires its own event cannot recurse without bound.
local function deferFire(inst, name, ...)
	local id = host.idOf(inst)
	local args = pack(...)
	local co = Sched.defer(function()
		Host.fireSignal(id, name, unpack(args, 1, args.n))
	end)
	local owner = ownerOf[co]
	if owner ~= nil then
		unclaim(co, owner)
	end
end

local luaMethods = {}
Host.luaMethods = luaMethods

luaMethods.WaitForChild = Host.waitForChild

function luaMethods.Fire(self, ...)
	-- Rune listeners hear it through the DataModel event queue; Luau ones
	-- directly, so tables and functions arrive intact.
	host.fired(self, ...)
	deferFire(self, "Event", ...)
end

function luaMethods.Invoke(self, ...)
	local fn = callback(host.idOf(self), "OnInvoke")
	if fn == nil then
		error("BindableFunction has no OnInvoke callback", 2)
	end
	return fn(...)
end

local function localPlayer()
	return game:GetService("Players").LocalPlayer
end

-- Calls this machine made that are waiting on an answer, keyed by the
-- invocation id the host handed back.
local pendingInvokes = {}

function luaMethods.FireServer(self, ...)
	-- A player sends it on. The host's own LocalScripts keep looping back,
	-- because on the host the server is already here.
	if host.remoteNetworked() and not host.remoteIsServer() then
		host.remoteFire(self, "Server", nil, ...)
		return
	end
	deferFire(self, "OnServerEvent", localPlayer(), ...)
end

function luaMethods.FireClient(self, player, ...)
	if host.remoteNetworked() then
		if not host.remoteIsServer() then
			error("FireClient can only be called from the server", 2)
		end
		if player ~= localPlayer() then
			host.remoteFire(self, "Client", player, ...)
			return
		end
	end
	-- Aimed at the host's own player, so it stays here.
	deferFire(self, "OnClientEvent", ...)
end

function luaMethods.FireAllClients(self, ...)
	if host.remoteNetworked() then
		if not host.remoteIsServer() then
			error("FireAllClients can only be called from the server", 2)
		end
		host.remoteFire(self, "All", nil, ...)
	end
	-- The host's own player is a client too, so it always hears it here.
	deferFire(self, "OnClientEvent", ...)
end

function luaMethods.InvokeServer(self, ...)
	if host.remoteNetworked() and not host.remoteIsServer() then
		if not isyieldable() then
			error("InvokeServer has to be called from a script thread", 2)
		end
		local id = host.remoteInvoke(self, ...)
		local pending = { done = false }
		pendingInvokes[id] = pending
		local waited = 0
		while not pending.done do
			waited = waited + Sched.wait(0)
			if waited >= host.remoteInvokeTimeout then
				pendingInvokes[id] = nil
				error("InvokeServer got no answer in " .. host.remoteInvokeTimeout .. " seconds", 2)
			end
		end
		pendingInvokes[id] = nil
		if pending.ok then
			return unpack(pending.results, 1, pending.results.n)
		end
		error(tostring(pending.err), 2)
	end
	local fn = callback(host.idOf(self), "OnServerInvoke")
	if fn == nil then
		error("RemoteFunction has no OnServerInvoke callback", 2)
	end
	return fn(localPlayer(), ...)
end

-- Typed remotes. The names go onto the instance as a comma separated string
-- property, so they replicate like any other property and the host can refuse
-- a call whose arguments do not match before a script sees it.
function luaMethods.SetArgumentTypes(self, types)
	if type(types) ~= "table" then
		error("SetArgumentTypes takes an array of type names", 2)
	end
	local parts = {}
	for i, name in ipairs(types) do
		parts[i] = tostring(name)
	end
	self.ArgumentTypes = table.concat(parts, ",")
end

-- A call that arrived from the session. `from` is the Player on a host and
-- nil on a player, where every call comes from the host. `invocation` is set
-- when the caller is parked on an answer.
function Host.deliverRemote(id, from, invocation, ...)
	if invocation == nil then
		if from ~= nil then
			Host.fireSignal(id, "OnServerEvent", from, ...)
		else
			Host.fireSignal(id, "OnClientEvent", ...)
		end
		return
	end
	local fn = callback(id, "OnServerInvoke")
	if fn == nil then
		host.remoteReply(invocation, from, false, "RemoteFunction has no OnServerInvoke callback")
		return
	end
	-- On its own thread, since OnServerInvoke may yield. The answer goes back
	-- whenever it returns, and an error travels as the message.
	local args = pack(...)
	Sched.spawn(function()
		local r = pack(pcall(fn, from, unpack(args, 1, args.n)))
		if r[1] then
			host.remoteReply(invocation, from, true, unpack(r, 2, r.n))
		else
			host.remoteReply(invocation, from, false, tostring(r[2]))
		end
	end)
end

-- The answer to an InvokeServer this machine made.
function Host.deliverReply(invocation, ok, err, ...)
	local pending = pendingInvokes[invocation]
	if pending == nil then
		return
	end
	pending.ok = ok
	pending.err = err
	pending.results = pack(...)
	pending.done = true
end

function luaMethods.InvokeClient(self, _player, ...)
	if host.remoteNetworked() and _player ~= nil and _player ~= localPlayer() then
		-- A player that never answers would hold this script for good.
		error("InvokeClient is not supported; use a RemoteEvent both ways", 2)
	end
	local fn = callback(host.idOf(self), "OnClientInvoke")
	if fn == nil then
		error("RemoteFunction has no OnClientInvoke callback", 2)
	end
	return fn(...)
end

-- CollectionService tag signals.
local tagSignals = {}
function Host.getTagSignal(tag, kind)
	local t = tagSignals[tag]
	if t == nil then
		t = { added = newSignal(), removed = newSignal() }
		tagSignals[tag] = t
	end
	return t[kind]
end
function Host.fireTagSignal(tag, kind, inst)
	local t = tagSignals[tag]
	if t then
		t[kind]:Fire(inst)
	end
end

-- ContextActionService: actions bound to keys or mouse buttons.
local actions = {}
local function inputMatches(bound, inputObject)
	local kc = inputObject.KeyCode
	local ut = inputObject.UserInputType
	for _, b in ipairs(bound) do
		if b == kc or b == ut then
			return true
		end
	end
	return false
end
Host.ContextActionService = {
	BindAction = function(_, name, fn, _createButton, ...)
		actions[name] = { fn = fn, inputs = { ... }, priority = 0 }
	end,
	BindActionAtPriority = function(_, name, fn, _createButton, priority, ...)
		actions[name] = { fn = fn, inputs = { ... }, priority = priority or 0 }
	end,
	UnbindAction = function(_, name)
		actions[name] = nil
	end,
	UnbindAllActions = function()
		for k in pairs(actions) do
			actions[k] = nil
		end
	end,
	GetAllBoundActionInfo = function()
		return {}
	end,
	SetTitle = function() end,
	SetImage = function() end,
	SetPosition = function() end,
}
function Host.dispatchActions(state, inputObject)
	for name, a in pairs(actions) do
		if inputMatches(a.inputs, inputObject) then
			callHandler(a.fn, name, state, inputObject)
		end
	end
end

-- MarketplaceService: products the creator sells in this simulation. The
-- catalog, prompts and receipts are the DataModel's commerce queues
-- (datamodel/commerce.rs); the engine buys and fulfills through the Commerce
-- API. Scripts name a product by its number, as Roblox scripts name a
-- developer product by its id.
local marketplaceSignals = {
	PromptProductPurchaseFinished = newSignal(),
	PromptGamePassPurchaseFinished = newSignal(),
	PromptPurchaseFinished = newSignal(),
}

-- A script that asks about products before the catalog arrives waits for it
-- (up to 10 s), as Roblox's calls wait for the web.
local function marketplaceReady()
	local status = host.commerceStatus()
	if status == "loading" and running() ~= nil and isyieldable() then
		for _ = 1, 100 do
			Sched.wait(0.1)
			status = host.commerceStatus()
			if status ~= "loading" then
				break
			end
		end
	end
	return status
end

-- A player who joined a multiplayer host has its passes read when it joins;
-- a script asking about them first waits for them (up to 10 s) rather than
-- answer false.
local function passesReady(user)
	if running() ~= nil and isyieldable() then
		for _ = 1, 100 do
			if not host.commercePassesPending(user) then
				return
			end
			Sched.wait(0.1)
		end
	end
end

Host.MarketplaceService = {
	PromptProductPurchase = function(_, player, productId)
		host.commercePrompt(player, productId, "consumable")
	end,
	PromptGamePassPurchase = function(_, player, passId)
		host.commercePrompt(player, passId, "pass")
	end,
	PromptPurchase = function(_, player, productId)
		host.commercePrompt(player, productId, "any")
	end,
	GetProductInfo = function(_, productId)
		local status = marketplaceReady()
		local info = host.commerceProduct(productId)
		if info == nil then
			if status ~= "ready" then
				error("MarketplaceService is " .. status, 2)
			end
			error("MarketplaceService:GetProductInfo(): this simulation has no product " .. tostring(productId), 2)
		end
		return info
	end,
	UserOwnsGamePassAsync = function(_, userId, passId)
		marketplaceReady()
		passesReady(userId)
		return host.commerceOwns(userId, passId)
	end,
	PlayerOwnsAsset = function(_, player, productId)
		marketplaceReady()
		passesReady(player)
		return host.commerceOwns(player, productId)
	end,
	PromptProductPurchaseFinished = marketplaceSignals.PromptProductPurchaseFinished,
	PromptGamePassPurchaseFinished = marketplaceSignals.PromptGamePassPurchaseFinished,
	PromptPurchaseFinished = marketplaceSignals.PromptPurchaseFinished,
}

-- Prompts that closed fire their signal. Receipts wait until a script sets
-- MarketplaceService.ProcessReceipt; then each runs on its own thread, so a
-- callback that yields (a DataStore write) holds up nothing else.
local function stepMarketplace()
	if host.commercePending() == 0 then
		return
	end
	local players = game:GetService("Players")
	for _, o in ipairs(host.commerceTakeOutcomes()) do
		if o.prompt == "consumable" then
			marketplaceSignals.PromptProductPurchaseFinished:Fire(o.userId, o.productId, o.purchased)
		elseif o.prompt == "pass" then
			marketplaceSignals.PromptGamePassPurchaseFinished:Fire(players:GetPlayerByUserId(o.userId), o.productId, o.purchased)
		else
			marketplaceSignals.PromptPurchaseFinished:Fire(players:GetPlayerByUserId(o.userId), o.productId, o.purchased)
		end
	end
	local processReceipt = callback(host.idOf(game:GetService("MarketplaceService")), "ProcessReceipt")
	if processReceipt == nil then
		return
	end
	for _, receipt in ipairs(host.commerceTakeReceipts()) do
		Sched.spawn(function()
			local ok, decision = pcall(processReceipt, receipt)
			if not ok then
				report(decision, running())
			end
			host.commerceDecision(receipt.PurchaseId, ok and decision == Host.Enum.ProductPurchaseDecision.PurchaseGranted)
		end)
	end
end

-- The Mouse (Player:GetMouse()). Live values come from the host each read.
local mouseSignals = {
	Button1Down = newSignal(),
	Button1Up = newSignal(),
	Button2Down = newSignal(),
	Button2Up = newSignal(),
	Move = newSignal(),
	WheelForward = newSignal(),
	WheelBackward = newSignal(),
	Idle = newSignal(),
}
Host.mouseSignals = mouseSignals
Host.mouse = setmetatable({}, {
	__type = "Instance",
	__index = function(_, key)
		local s = mouseSignals[key]
		if s then
			return s
		end
		return host.mouseGet(key)
	end,
	__newindex = function(_, key, value)
		host.mouseSet(key, value)
	end,
	__tostring = function()
		return "Mouse"
	end,
})

-- ─────────────────────────────────────────────────────────────────────────
-- Frame entry points (called by the Rust host)
-- ─────────────────────────────────────────────────────────────────────────

local RunServiceSignals = {
	Stepped = newSignal(),
	Heartbeat = newSignal(),
	RenderStepped = newSignal(),
	PreSimulation = newSignal(),
	PostSimulation = newSignal(),
	PreRender = newSignal(),
}
Host.RunServiceSignals = RunServiceSignals

local renderBindings = {}
function Host.bindToRenderStep(name, priority, fn)
	renderBindings[name] = { priority = priority or 0, fn = fn }
end
function Host.unbindFromRenderStep(name)
	renderBindings[name] = nil
end

-- Threads that will never resume, or have waited on nothing for 5 s: one
-- warning each, where the thread waits.
local function reportStuck()
	if #orphans > 0 then
		local list = orphans
		orphans = {}
		for _, o in ipairs(list) do
			if status(o.co) == "suspended" then
				warnStuck(o.co, "Infinite yield: this thread waits on " .. o.what .. ", which was destroyed, so it never resumes")
			end
		end
	end
	if next(rawYields) ~= nil then
		for co, since in pairs(rawYields) do
			if parked[co] or status(co) ~= "suspended" then
				rawYields[co] = nil
			elseif Sched.now - since >= 5 then
				rawYields[co] = nil
				warnStuck(co, "Infinite yield possible: this thread called coroutine.yield() and nothing has resumed it for 5 s")
			end
		end
	end
end

function Host.frame(now, dt)
	local sorted = nil
	for name, b in pairs(renderBindings) do
		sorted = sorted or {}
		tinsert(sorted, b)
	end
	if sorted then
		table.sort(sorted, function(a, b)
			return a.priority < b.priority
		end)
		for _, b in ipairs(sorted) do
			callHandler(b.fn, dt)
		end
	end
	RunServiceSignals.PreRender:Fire(dt)
	RunServiceSignals.RenderStepped:Fire(dt)
	RunServiceSignals.PreSimulation:Fire(dt)
	RunServiceSignals.Stepped:Fire(now, dt)
	Sched.step(now)
	stepMarketplace()
	stepTweens(dt)
	RunServiceSignals.PostSimulation:Fire(dt)
	RunServiceSignals.Heartbeat:Fire(dt)
	Host.flushDeferred()
	reportStuck()
end

-- Tear everything down for Stop.
function Host.reset()
	for _, e in ipairs(Sched.sleeping) do
		if e.co and e.co ~= running() and status(e.co) ~= "dead" then
			pcall(close, e.co)
		end
	end
	Sched.sleeping = {}
	Sched.deferred = {}
	orphans = {}
	for co in pairs(rawYields) do
		rawYields[co] = nil
	end
	for id in pairs(instanceSignals) do
		Host.dropSignals(id)
	end
	for _, sig in pairs(RunServiceSignals) do
		sig:DisconnectAll()
	end
	for _, sig in pairs(marketplaceSignals) do
		sig:DisconnectAll()
	end
	for k in pairs(activeTweens) do
		activeTweens[k] = nil
	end
	for k in pairs(renderBindings) do
		renderBindings[k] = nil
	end
end

return Host
