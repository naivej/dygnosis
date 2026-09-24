// inventory: p_pac_quiet_deterministic_adhoc_call
var y;
model;
  y=0;
end;
deterministic_trends;
  y(ghost(-1));
end;
