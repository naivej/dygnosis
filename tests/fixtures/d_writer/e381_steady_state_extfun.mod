// inventory: e381_steady_state_extfun
var c;
external_function(name='ef', nargs=1);
model;
c = steady_state(ef(c));
end;
