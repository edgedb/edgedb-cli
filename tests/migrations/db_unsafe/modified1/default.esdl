module default {
    type Type1 {
        # implicit cast
        property always_integer -> int64;
        # existing cast
        property string_int -> int;
        # no cast (drop, create)
        property int_dur -> duration;
        # make required
        required property opt_req -> int32;
        # make single
        property multi_single -> str;
    }
}
