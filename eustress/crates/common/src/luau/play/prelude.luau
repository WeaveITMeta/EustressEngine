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

local function report(err, thread)
	local msg = tostring(err)
	local tb = debug.traceback(thread, msg)
	host.output("error", tb or msg)
end
Host.report = report

-- ─────────────────────────────────────────────────────────────────────────
-- Scheduler
-- ─────────────────────────────────────────────────────────────────────────

local Sched = {
	now = 0,
	sleeping = {}, -- { co, at, started, args, cancelled }
	deferred = {}, -- { co, args, cancelled }
}
Host.Sched = Sched

local function safe_resume(co, ...)
	if co == nil or status(co) == "dead" then
		return
	end
	local ok, err = resume(co, ...)
	if not ok then
		report(err, co)
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
		acquireAndCall(yield())
	end
end
local function callHandler(fn, ...)
	if freeRunner == nil then
		freeRunner = create(runnerLoop)
		local ok, err = resume(freeRunner, fn, ...)
		if not ok then
			report(err, freeRunner)
			freeRunner = nil
		end
		return
	end
	local runner = freeRunner
	local ok, err = resume(runner, fn, ...)
	if not ok then
		report(err, runner)
		if freeRunner == runner then
			freeRunner = nil
		end
	end
end
Host.callHandler = callHandler

function Sched.spawn(fn, ...)
	local co = (type(fn) == "thread") and fn or create(fn)
	safe_resume(co, ...)
	return co
end

function Sched.defer(fn, ...)
	local co = (type(fn) == "thread") and fn or create(fn)
	tinsert(Sched.deferred, { co = co, args = pack(...) })
	return co
end

function Sched.delay(sec, fn, ...)
	local co = (type(fn) == "thread") and fn or create(fn)
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
				callHandler(c._fn, ...)
			end
		end
	end
	local waiters = self._waiters
	if #waiters > 0 then
		self._waiters = {}
		for _, co in ipairs(waiters) do
			safe_resume(co, ...)
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

function Host.dropSignals(id)
	local byName = instanceSignals[id]
	if byName then
		for _, sig in pairs(byName) do
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
	for prop in pairs(self._goals) do
		self._from[prop] = self.Instance[prop]
	end
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
				for prop, goal in pairs(tw._goals) do
					local ok, err = pcall(function()
						inst[prop] = lerpValue(tw._from[prop], goal, alpha)
					end)
					if not ok then
						host.output("warn", "Tween: " .. tostring(err))
					end
				end
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
			host.output("warn", "Infinite yield possible on '" .. parent:GetFullName() .. ":WaitForChild(\"" .. tostring(name) .. "\")'")
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
	Sched.defer(function()
		Host.fireSignal(id, name, unpack(args, 1, args.n))
	end)
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

function luaMethods.FireServer(self, ...)
	deferFire(self, "OnServerEvent", localPlayer(), ...)
end

function luaMethods.FireClient(self, _player, ...)
	deferFire(self, "OnClientEvent", ...)
end

function luaMethods.FireAllClients(self, ...)
	deferFire(self, "OnClientEvent", ...)
end

function luaMethods.InvokeServer(self, ...)
	local fn = callback(host.idOf(self), "OnServerInvoke")
	if fn == nil then
		error("RemoteFunction has no OnServerInvoke callback", 2)
	end
	return fn(localPlayer(), ...)
end

function luaMethods.InvokeClient(self, _player, ...)
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

-- AnimationTrack stand-in: accepts the calls ported scripts make.
function Host.stubAnimationTrack()
	local track = {
		IsPlaying = false,
		Length = 0,
		Speed = 1,
		TimePosition = 0,
		WeightCurrent = 1,
		Looped = false,
		Priority = Host.Enum.AnimationPriority.Core,
		Stopped = newSignal(),
		Ended = newSignal(),
		DidLoop = newSignal(),
		KeyframeReached = newSignal(),
	}
	function track:Play()
		self.IsPlaying = true
	end
	function track:Stop()
		self.IsPlaying = false
		self.Stopped:Fire()
	end
	function track:AdjustSpeed(s)
		self.Speed = s
	end
	function track:AdjustWeight() end
	function track:GetMarkerReachedSignal()
		return newSignal()
	end
	return track
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
	stepTweens(dt)
	RunServiceSignals.PostSimulation:Fire(dt)
	RunServiceSignals.Heartbeat:Fire(dt)
	Host.flushDeferred()
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
	for id in pairs(instanceSignals) do
		Host.dropSignals(id)
	end
	for _, sig in pairs(RunServiceSignals) do
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
