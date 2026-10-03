// inventory: E481 exogenous steady_state_model target has incorrect type.
var y;
varexo e;
model;
  y=e;
end;
steady_state_model;
  e=1;
  y=0;
end;
