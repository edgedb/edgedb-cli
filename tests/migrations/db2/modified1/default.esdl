module default {
    type X;
    type Bar { link spam -> Spam };
    type Spam { link bar -> Bar };
};
