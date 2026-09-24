// inventory: p_pac_e279_deterministic_external
var y;
external_function(name=helper);
model;
  y=0;
end;
deterministic_trends;
  y(helper);
end;
